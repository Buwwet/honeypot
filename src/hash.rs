use fancy_regex::Regex;
// This module contains the logic need to identify images from users and hash them.
use serenity::{all::{Context, EventHandler, Message}, async_trait};
use sqlx::{FromRow, Pool, Postgres, query, query_as, types::chrono::{self, DateTime, NaiveDate, Utc}};


const IMAGE_TYPES: [&'static str; 4] = ["image/png", "image/jpg", "image/jpeg", "image/webp"];



/// A struct to hold both Attachments and ImageEmbeds
#[derive(Debug)]
pub struct UserFile {
    pub url: String,
    pub content_type: Option<String>
}

impl UserFile {
    // Gets the attachments, embeds and links from a website.
    pub fn from_message(msg: &Message) -> Vec<Self> {
        let mut files = vec![];

        // First attachments
        for attachment in &msg.attachments {
            files.push(UserFile {
                url: attachment.url.clone(),
                content_type: attachment.content_type.clone(),
            });
        }

        for embed in &msg.embeds {
            if let Some(img) = &embed.image {
                files.push(UserFile {
                    url: img.url.to_string(),
                    content_type: Some("image/webp".to_string())
                });
            }
        }

        // Now we need to parse the images held within links, as these actually show up!
        // This regex pattern matches images
        let re = Regex::new(r"(http|https)[a-z]*\S+").unwrap();
        for capture in re.captures_iter(&msg.content) {
            // Get the whole match
            let url = capture.unwrap().get(0).unwrap().as_str();
            if url.contains('.') {
                // Now, most of the time images have some extra data after ?, this hides the real extension.
                // We want what lies between . and ?
                let mut content_type = format!("image/{}", url.split('.').last().unwrap());
                if content_type.contains('?') {
                    // Get the first one.
                    content_type = content_type.split('?').next().unwrap().to_string(); 
                }


                files.push(UserFile {
                    url: url.to_string(),
                    content_type: Some(content_type)
                });
            }
        }
        
        return files;
    }

    fn is_image(&self) -> bool {
        if self.content_type.is_none() {
            return false; // No point in checking.
        }

        for img_type in IMAGE_TYPES {
            if self.content_type.as_ref().unwrap() == img_type {
                return true;
            }
        }
        return false;
    }

}

#[derive(Debug, FromRow)]
struct HashEntry {
    pub hash: String,
    pub instances: i32,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}

impl HashEntry {
    /// Create a new wrapper for our hash
    pub fn new(hash: String) -> Self {
        let today = chrono::Utc::now();
        Self {
            hash, instances: 1, first_seen: today, last_seen: today,
        }
    }
}

pub enum HashBotError {
    AttachmentAlreadyExists {hash: String},
    InvalidAttachmentFormat {format: String},
    HashNotFound {hash: String},
}

impl HashBotError {
    pub fn to_message(&self) -> String {
        match &self {
            Self::AttachmentAlreadyExists { hash } =>
                format!("Tried to add an already existing hash: {}", hash),
            Self::InvalidAttachmentFormat { format } =>
                format!("Tried to hash a file with an unsupported format: {}", format),
            Self::HashNotFound { hash } =>
                format!("Requested hash does not exist: {}", hash),
        }
    }
}

pub struct HashBot {
    pool: Pool<Postgres>,
    client: reqwest::Client,
}

impl HashBot {
    pub fn new(pool: Pool<Postgres>) -> Self {
        Self {
            client: reqwest::Client::new(),
            pool
        }
    }

    // Downloads the given image to RAM and performs aHash
    async fn hash_image(&self, url: &str) -> Result<String, ()> {
        let img_data = self.client.get(url).send().await
            .expect("Expected to get valid image from URL").bytes().await.unwrap();

        let img = image::load_from_memory(&img_data).expect("Expected to load image from body data");

        // Now we can run imagehash-rs's average hash
        let hash = imagehash::AverageHash::new()
            .with_hash_size(8, 8).with_image_size(8, 8).hash(&img).to_string();

        Ok(hash)
    }

    /// Returns the record of a given bot hash image if found in the database
    async fn db_get_entry(&self, hash: &str) -> Option<HashEntry> {
        query_as!(HashEntry, "SELECT * FROM bot_images WHERE hash=$1", hash).fetch_optional(&self.pool).await
            .expect("Expected to plausibly get record from database")
    }

    /// Adds a hash to the table.
    async fn db_add_entry(&self, entry: HashEntry) {
        query!("INSERT INTO bot_images(hash, instances, first_seen, last_seen) VALUES($1, $2, $3, $4)",
            entry.hash,
            entry.instances,
            entry.first_seen,
            entry.last_seen,
        ).execute(&self.pool)
            .await.expect("Expected to add an entry to the db");
    }

    /// Updates the instances and last seen of a given entry.
    async fn db_update_entry(&self, entry: &HashEntry) {
        query!("UPDATE bot_images SET instances=$2, last_seen=$3 WHERE hash=$1",
            entry.hash,
            entry.instances,
            entry.last_seen,
        ).execute(&self.pool).await.expect("Expected to modify value of existing hash.");
    }

    /// Removes the given entry
    async fn db_delete_entry(&self, entry: &String) {
        query!("DELETE FROM bot_images WHERE hash=$1",
            entry
        ).execute(&self.pool).await.expect("Expected to modify value of existing hash.");
    }

    pub async fn delete_hash(&self, hash: &String) -> Result<(), HashBotError> {
        // Check if it exists.
        if self.db_get_entry(&hash).await.is_some() {
            // It does, let's remove it.
            self.db_delete_entry(&hash).await;
            return Ok(());
        } else {
            return Err(HashBotError::HashNotFound { hash: hash.clone() })
        }
    }

    /// Hashes and adds an attachment to the database.
    pub async fn add_attachment(&self, attachment: &UserFile) -> Result<String, HashBotError> {
        // Safeguards for file formats.
        if !attachment.is_image() {return Err(HashBotError::InvalidAttachmentFormat { format: attachment.content_type.as_ref().unwrap().clone() });}

        let hash = self.hash_image(&attachment.url).await.expect("Expected to hash attachment");

        // Don't add it if it already exists.
        if self.db_get_entry(&hash).await.is_some() {return Err(HashBotError::AttachmentAlreadyExists { hash: hash });}

        // It's really new!
        let entry = HashEntry::new(hash.clone());
        self.db_add_entry(entry).await;

        println!("Added HASH {}", &hash);

        Ok(hash)
    }
}

#[async_trait]
impl EventHandler for HashBot {
    async fn message(&self, ctx: Context, honeypot_msg: Message) {

        // TODO: filter out messages from channels we don't want.

        let mut user_is_bot = false;
        let attachments = UserFile::from_message(&honeypot_msg);
        for attachment in attachments {
            // TODO: Check for the active member permision before going through.
            println!("user sent: {:?}", attachment.content_type);

            if !attachment.is_image() {continue;}

            let hash = self.hash_image(&attachment.url).await.expect("Expected to get the hash of an attachment");
            println!("HASH: {:?}", hash);
            println!("ENTRY: {:?}", self.db_get_entry(&hash).await);

            // TODO: actual banning and stuff
        }
    }
}