use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use tokio::io;
use tokio::io::{AsyncBufReadExt};
use tokio::spawn;
use regex::Regex;

mod network;
mod rendezvous_server;

#[tokio::main]
async fn main() {
    let (mut client, network_event_loop) =
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
            // "connect" => {
            //     client.connect("/ip4/10.0.0.32/tcp/0".parse().unwrap()).await;
            // },
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
                    let mut filenames: Vec<String> = Vec::new();
                    filenames.push(filename.to_string());
                    
                    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
                    files.insert(filename.to_string(), file);
                    client.offer_files(filenames, files).await;
                }
            }
            "get" => {
                if args.len() > 1 {
                    let username = args.get(1).unwrap().to_string();
                    client.get_offerings(username).await;
                }
            }
            "trade" => {
                if args.len() > 3 {
                    let username = args.get(1).unwrap().to_string();
                    let my_file = args.get(2).unwrap().to_string();
                    let their_file = args.get(3).unwrap().to_string();
                    let response = client.offer_trade(my_file.clone(), their_file, username.clone()).await.unwrap();
                    if response.accepted {
                        println!("Trade offer accepted, downloaded file");
                        let file = response.file;
                        let filename = response.filename;
                        let path = network::OUTPUT_PATH.to_owned() + filename.as_str();
                        match fs::create_dir(network::OUTPUT_PATH) {
                            Ok(_) => {}
                            Err(_) => {}
                        }
                        let mut location = File::create(path.clone()).unwrap();
                        location.write_all(&*file).unwrap();

                        client.send_file(username, my_file).await;
                        println!("Sent return file");
                    } else {
                        println!("trade failed.");
                    }
                }
            },
            "accept" => {
                client.offer_response(true).await;
            },
            "deny" => {
                client.offer_response(false).await;
            },
            "discovery" => {
                if args.len() > 1 {
                    let address = args.get(1).unwrap().to_string();
                    match client.connect_rendezvous(address).await {
                        Ok(_) => println!("Connected to rendezvous server and discovery initiated"),
                        Err(e) => println!("Failed to connect to rendezvous server: {:?}", e)
                    }
                } else {
                    println!("Usage: discovery <server_address>");
                    println!("Example: discovery /ip4/127.0.0.1/tcp/62649");
                }
            },
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