use std::{io::Write, process::Command};

use serenity::{client::Context, futures::StreamExt};

use crate::{check_msg, database, HasBossMusic};

pub async fn download_attachment(
    ctx: &Context,
    url: &str,
    boss_music_file_path: &str,
    file_name: &str,
    msg: &serenity::model::channel::Message,
    user_id: &u64,
) {
    // the attachment data
    let response = reqwest::get(url).await.expect("attchment request failed");
    // Create the file that we will store the attachment in (no extenstion). I will be later deleted
    let mut file = std::fs::File::create(format!("{}{}", boss_music_file_path, file_name))
        .expect("cannot create file");
    let mut stream = response.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = match item {
            Ok(data) => data,
            Err(_) => {
                println!("Error while downloading file");
                return;
            }
        };
        match file.write(&chunk) {
            Ok(_) => {}
            Err(_) => {
                println!("Error while writing to file");
                return;
            }
        }
    }
    // // ??
    // let mut content = Cursor::new(response.bytes().await.expect("??"));
    // // copy memory content to the file
    // std::io::copy(&mut content, &mut file).expect("cannot copy file");

    let output = Command::new("ffprobe")
        .arg(format!("{}{}", boss_music_file_path, file_name))
        .args(["-show_streams", "-show_format"])
        .output()
        .expect("failed to laucnh ffprobe");
    let s = String::from_utf8_lossy(&output.stdout);
    for line in s.lines() {
        if line.starts_with("duration=") {
            let after: Vec<&str> = line.split('=').collect();
            let float: f32 = after[1].parse().unwrap();
            println!("duration {}", float);
        } else if line.starts_with("probe_score=") {
            let after: Vec<&str> = line.split('=').collect();
            let int: i32 = after[1].parse().unwrap();
            println!("score {}", int);
        } else if line.starts_with("codec_type=") {
            let after: Vec<&str> = line.split('=').collect();
            let codec_type = after[1];
            println!("codec {}", codec_type);
        }
    }

    let command1 = match Command::new("ffmpeg")
        // .arg(format!("-ss {}", 0))
        .arg("-ss")
        .arg("0")
        .arg("-t")
        .arg("0")
        .arg("-i")
        .arg(format!("{}{}", boss_music_file_path, file_name))
        .arg("-y")
        .args(["-c:a", "libopus", "-b:a", "96k"])
        .arg(format!("{}{}.ogg", boss_music_file_path, file_name))
        .output()
    {
        Ok(result) => {
            check_msg(msg.reply(ctx, "Ok piss").await);
            result
        }
        Err(err) => {
            println!("Cannot convert");
            check_msg(msg.reply(ctx, "Cannot convert file").await);
            return;
        }
    };
    // remove temporary file
    match std::fs::remove_file(format!("{}{}", boss_music_file_path, file_name)) {
        Ok(_) => {}
        Err(err) => {
            println!("Error when remove file {}", err)
        }
    };

    {
        let mut data = ctx.data.write().await;
        let has_boss_music = data.get_mut::<HasBossMusic>().unwrap();
        has_boss_music.insert(*user_id, Some(format!("{}.ogg", file_name)));
    }

    database::voice::add_user_boss_music(ctx, user_id, file_name).await;

    println!("err {}", String::from_utf8_lossy(&command1.stderr));
    println!("out {}", String::from_utf8_lossy(&command1.stdout));
    println!("status {}", command1.status.to_string());
}

pub async fn download_youtube(
    ctx: &Context,
    url: &str,
    boss_music_file_path: &str,
    file_name: &str,
    msg: &serenity::model::channel::Message,
    user_id: &u64,
) {
}
