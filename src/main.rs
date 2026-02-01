use std::time::Duration;

use captcha_rs::CaptchaBuilder;
use serenity::all::{
    CreateAttachment, CreateButton, CreateInteractionResponse, CreateInteractionResponseMessage,
    CreateMessage,
};
use serenity::async_trait;
use serenity::model::prelude::*;
use serenity::prelude::*;

use crate::hash::HashBot;

mod hash;
mod commands;

struct Bot {
    channel: ChannelId,
    ban_message: String,
}

#[async_trait]
impl EventHandler for Bot {
    async fn message(&self, ctx: Context, honeypot_msg: Message) {
        if honeypot_msg.channel_id == self.channel {
            let member = honeypot_msg
                .member(&ctx.http)
                .await
                .expect("user isn't a member");

            // if user is a moderator, return early
            let guild_channel = honeypot_msg
                .channel(&ctx.http)
                .await
                .expect("couldn't find channel")
                .guild()
                .expect("couldn't infer guild channel");

            let perms = honeypot_msg
                .guild(&ctx.cache)
                .expect("couldn't find guild")
                .user_permissions_in(&guild_channel, &member);

            if perms.intersects(
                Permissions::KICK_MEMBERS | Permissions::BAN_MEMBERS | Permissions::ADMINISTRATOR,
            ) {
                return;
            }

            let builder = CreateMessage::new()
                .content(&self.ban_message)
                .button(CreateButton::new("start_captcha").label("Start Captcha"));

            let dm_msg = honeypot_msg.author.direct_message(&ctx.http, builder).await;

            member
                .ban_with_reason(&ctx.http, 1, "sent message in honeypot channel")
                .await
                .expect("couldn't ban member");

            if let Ok(dm_msg) = dm_msg {
                start_captcha(ctx, honeypot_msg, dm_msg, member).await;
            }
        }
    }
}

fn generate_captcha() -> (String, Vec<u8>) {
    let captcha = CaptchaBuilder::new()
        .length(6)
        .width(220)
        .height(80)
        .dark_mode(true)
        .complexity(5)
        .build();

    (captcha.text.clone(), captcha.to_bytes())
}

async fn start_captcha(ctx: Context, honeypot_msg: Message, dm_msg: Message, member: Member) {
    let interaction = match dm_msg
        .await_component_interaction(&ctx.shard)
        .channel_id(dm_msg.channel_id)
        .author_id(honeypot_msg.author.id)
        .custom_ids(vec!["start_captcha".to_string()])
        .timeout(Duration::from_secs(300))
        .await
    {
        Some(i) => i,
        None => return,
    };

    let (result, image) = tokio::task::spawn_blocking(generate_captcha)
        .await
        .expect("couldn't generate captcha");

    let builder = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .content("Send a message containing the content of this image.\nYou have one try.")
            .add_file(CreateAttachment::bytes(image, "captcha.png")),
    );

    interaction
        .create_response(&ctx.http, builder)
        .await
        .expect("couldn't create interaction response");

    let response = match honeypot_msg
        .channel_id
        .await_reply(&ctx.shard)
        .channel_id(dm_msg.channel_id)
        .author_id(honeypot_msg.author.id)
        .timeout(Duration::from_secs(300))
        .await
    {
        Some(m) => m,
        None => return,
    };

    response
        .reply(
            &ctx.http,
            match response.content.contains(&result) {
                true => {
                    member.unban(&ctx.http).await.expect("couldn't unban");
                    "Correct. You have been unbanned."
                }
                false => {
                    "Incorrect. Submit a ban appeal or contact the admins of the server."
                }
            },
        )
        .await
        .expect("couldn't reply");
}

#[tokio::main]
async fn main() {
    let token = std::env::var("HONEYPOT_TOKEN").expect("HONEYPOT_TOKEN missing");

    let bot = Bot {
        channel: ChannelId::new(
            std::env::var("HONEYPOT_CHANNEL")
                .expect("HONEYPOT_CHANNEL missing")
                .parse()
                .expect("invalid channel id"),
        ),
        ban_message: std::env::var("HONEYPOT_BAN_MESSAGE")
            .unwrap_or_else(|_| concat!(
                "You have been automatically banned due to sending scam messages.\n",
                "This is likely due to a compromised account.\n",
                "If you recover your account, please make a ban appeal at <https://figuramc.org/forms/user/unban>\n\n",
                "If you sent a message by mistake, you have 5 minutes to solve a captcha.\n",
                "To solve the captcha, you need to:\n",
                "- join this server: https://discord.gg/5UWBcGWGac\n",
                "- click on the button below"
            ).to_string()),
    };

    // Connect to our database!
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL missing")).await.expect("Expected to connect to honeypot-hash on postgres");
    let hash_bot = HashBot::new(pool.clone());

    // Setup commands with poise.
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![commands::ping(), commands::hash_message_attachments(), commands::remove_hash()],
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                // TODO: we're doing this just to access our pool and DB helper inside commands. We might need a global mutex eventually instead.
                Ok(HashBot::new(pool))
            })
        }).build();

    let mut client = Client::builder(
        &token,
        GatewayIntents::GUILDS
            | GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT,
    )
    .framework(framework)
    .event_handler(bot)
    .event_handler(hash_bot)
    .await
    .expect("error creating client");

    client.start().await.unwrap();
}
