// Commands using poise's beautiful macros.

use serenity::{all::{ChannelId, CreateMessage, Mentionable, Message, User}, futures::future::{join_all}};

use crate::hash::{HashBot, UserFile};

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, HashBot, Error>;

pub async fn send_moderator_message(ctx: &Context<'_>, content: &str) {
    ctx.data().get_moderation_channel_id().send_message(ctx, CreateMessage::new().content(content)).await
        .expect("Expected to send message to the moderator channel");
}


/// A helper command that adds the sent attachments to our hash database and provides some info in the moderator's channel
async fn add_attachment(ctx: &Context<'_>, attachment: UserFile, message: &Message) {
    let res = ctx.data().add_attachment(&attachment).await;
    if let Err(err) = res {
        send_moderator_message(ctx, 
            &format!("ERROR ADDING ATTACHMENT {} in {}: {}", ctx.author().mention(), message.link(), &err.to_message())
        ).await;
    } else {
        // Hash image was added successfuly.
        send_moderator_message(ctx, 
            &format!("{} added hash {} to database > {}", ctx.author().mention(), res.ok().unwrap(), attachment.url)
        ).await;
    }
}


/// Test command
// It seems that context_menu_commands can either recieve a user or msg as their only parameter
#[poise::command(slash_command, context_menu_command = "ping em")]
pub async fn ping(ctx: Context<'_>, user: User) -> Result<(), Error> {
    println!("{} RAN OUR COMMAND!!!", ctx.author().name);
    poise::say_reply(ctx, "That's so cool").await.expect("Expected to send a cool headsup");

    
    // Lets try to send a moderator command
    send_moderator_message(&ctx, &format!("{} pinged {}!!!", ctx.author().name, user.mention())).await;

    Ok(())
}

// Real Commands

/// Hash Message Attachments
/// \n
/// Does NOT ban the user.
#[poise::command(context_menu_command = "Hash Message Attachments", ephemeral)] //TODO: permisions.
pub async fn hash_message_attachments(ctx: Context<'_>, msg: Message) -> Result<(), Error> {
    
    // Handle each attachment.
    let attachments = UserFile::from_message(&msg);

    let mut futures = Vec::with_capacity(attachments.len());
    for attachment in attachments {
        futures.push(add_attachment(&ctx, attachment, &msg));
    }
    // Wait for the futures together
    join_all(futures.into_iter()).await;

    ctx.say(":thumbsup:").await.unwrap();

    Ok(())
}

/// Ban & Hash bot
/// \n
/// Bans the user and hashes their attachments.
#[poise::command(context_menu_command = "Hash&Ban Bot", ephemeral)]
pub async fn hash_and_ban(ctx: Context<'_>, msg: Message) -> Result<(), Error> {
    // Handle each attachment.
    let attachments = UserFile::from_message(&msg);

    let mut futures = Vec::with_capacity(attachments.len());
    for attachment in attachments {
        futures.push(add_attachment(&ctx, attachment, &msg));
    }
    // Wait for the futures together
    join_all(futures.into_iter()).await;

    // Alright, now we can ban the member.
    // For some reason in context menus we don't get access to the message's author
    //println!("GUILD {:?}", ctx.guild_id().unwrap().member(ctx, msg.author.id).await.unwrap().display_name());
    //println!("MEMBER {:?}", ctx.author_member().await.unwrap().display_name());

    let member = ctx.guild_id().expect("Expected to get guild")
        .member(ctx, msg.author.id).await.expect("Expected to get bot member");

    member 
        .ban_with_reason(&ctx, 1, "begone!")
        .await
        .expect("couldn't ban member");

    ctx.say("User hashed and banned").await.unwrap();

    Ok(())
}

/// Removes a hash from the database
#[poise::command(slash_command, ephemeral)] //TODO: permisions.
pub async fn remove_hash(ctx: Context<'_>, hash: String) -> Result<(), Error> {

    let res = ctx.data().delete_hash(&hash).await;
    if res.is_ok() {
        ctx.say("Removed it, boss").await.unwrap();
    } else {
        ctx.say("Oh no...").await.unwrap();
        send_moderator_message(&ctx, &format!("ERROR REMOVING HASH: {}", res.err().unwrap().to_message())).await;
    }

    Ok(())
}

