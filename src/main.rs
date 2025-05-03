use std::fs;
use std::path::Path;
use tokio::io;
use tokio::io::{AsyncBufReadExt};
use tokio::spawn;
use regex::Regex;

mod network;

#[tokio::main]
async fn main() {
    let (mut client, mut network_events, network_event_loop) =
    network::new(None).await.unwrap();

    spawn(network_event_loop.run());

    let mut stdin = io::BufReader::new(io::stdin()).lines();

    loop {
        
        let line = stdin.next_line().await.unwrap();
        let args = split_string(&line.unwrap().as_str());
        let cmd = if let Some(cmd) = args.get(0) { cmd } else {
            println!("No command given");
            return;
        };
        
        match cmd.as_str() {
            "msg" => {
                if args.len() > 1 {
                    let message = args.get(1).unwrap().to_string();
                    client.send_message(message).await;
                }
            },
            "reg" => {
                if args.len() > 1 {
                    let username = args.get(1).unwrap().to_string();
                    client.register(username).await;
                }
            },
            "topic" => {
                if args.len() > 1 {
                    let topic = args.get(1).unwrap().to_string();
                    client.change_topic(topic).await;
                }
            },
            "connect" => {
                client.connect("/ip4/10.0.0.32/tcp/0".parse().unwrap()).await;
            },
            "dial" => {
                if args.len() > 1 {
                    let username = args.get(1).unwrap().to_string();
                    client.dial(username).await;
                }
            },
            "dm" => {
                if args.len() > 2 {
                    let username = args.get(1).unwrap().to_string();
                    let message = args.get(2).unwrap().to_string();
                    client.dm(username, message).await;
                }
            },
            "offer" => {
                // println!("{}", std::env::current_dir().unwrap().to_str().unwrap());
                if args.len() > 1 {
                    //TODO Fix bad file exiting loop
                    let filepath = args.get(1).unwrap().to_string();
                    let file = match fs::read(filepath.clone()) {
                        Ok(file) => {file}
                        Err(_) => {println!("Failed to load file"); return}
                    };
                    // if file.len() == 0 {println!("Failed to load file"); return}
                    let filename = match Path::new(&filepath).file_name() {
                        None => {println!("Failed to load file"); return}
                        Some(name) => {name.to_str().unwrap()}
                    };
                    let mut files: Vec<String> = Vec::new();
                    files.push(filename.to_string());
                    client.offer_files(files).await;
                }
            }
            "get" => {
                if args.len() > 1 {
                    let username = args.get(1).unwrap().to_string();
                    client.get_offerings(username).await;
                }
            }
            _ => {}
        }
    }
}

fn split_string(input: &str) -> Vec<String> {
    let re = Regex::new(r#""([^"]*)"|\S+"#).unwrap();
    re.captures_iter(input)
        .map(|cap| cap.get(0).unwrap().as_str().to_string())
        .collect()
}