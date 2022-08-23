#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use]
extern crate rocket;
extern crate core;

use chrono::{DateTime, Utc};
use rocket::http::RawStr;
use rocket::request::Form;
use rocket::response::{NamedFile, Redirect};
use rocket::State;
use rocket_contrib::json;
use rocket_contrib::json::{Json, JsonValue};
use rocket_contrib::templates::Template;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs;
use std::fs::DirEntry;
use std::ops::Deref;
use std::path::Path;
use std::sync::Mutex;

#[derive(FromForm)]
struct WateringInput {
    time_s: String,
}

//TODO:
// * TLS
// * login
// * Other routes (telemetry, watering done, upload pic)

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Command {
    action: String,
    duration: Option<u8>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
struct Telemetry {
    moisture: Option<u16>,
    water_level: Option<u8>,
    temperature: Option<i16>,
}

#[derive(Serialize, Deserialize)]
struct Data {
    commands: Vec<Command>,
    telemetry: Telemetry,
    last_watering_time: Option<String>,
    last_watering_status: Option<bool>,
    last_sent: String, //That is for debug only
}

#[get("/commands")]
fn commands(state: State<Mutex<Data>>) -> Json<Vec<Command>> {
    let mut data = state.lock().unwrap();
    let ret = Json(data.commands.clone());
    data.commands.clear();

    ret

    /*if data.last_sent == "telemetry" {
        data.last_sent = "watering".to_string();
        Json(vec![Command {
            duration: Some(1),
            action: "watering".to_string(),
        }])
    } else {
        data.last_sent = "telemetry".to_string();
        Json(vec![Command {
            duration: None,
            action: "telemetry".to_string(),
        }])
    }*/
}

#[put("/watering_status/<status>")]
fn watering_status(status: bool, state: State<Mutex<Data>>) -> JsonValue {
    let mut data = state.lock().unwrap();

    data.last_watering_status = Some(status);
    data.last_watering_time = Some(Utc::now().to_string());

    json!({ "status": "ok" })
}

#[post("/telemetry", format = "json", data = "<telemetry>")]
fn telemetry(telemetry: Json<Telemetry>, state: State<Mutex<Data>>) -> JsonValue {
    let mut data = state.lock().unwrap();
    data.telemetry.moisture = telemetry.moisture;
    data.telemetry.water_level = telemetry.water_level;
    data.telemetry.temperature = telemetry.temperature;

    json!({ "status": "ok" })
}

#[get("/last_pic.jpeg")]
fn last_pic(_state: State<Mutex<Data>>) -> Option<NamedFile> {
    let paths = match fs::read_dir("/home/detlev/pics/") {
        Ok(p) => p,
        Err(_) => {
            println!("No dir found");
            return None;
        }
    };

    let mut paths: Vec<DirEntry> = paths
        .filter_map(|d| if let Ok(r) = d { Some(r) } else { None })
        .collect();
    paths.sort_by(|a, b| {
        if a.file_name() < b.file_name() {
            Ordering::Less
        } else if a.file_name() > b.file_name() {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    });

    if paths.is_empty() {
        println!("No file found");
        return None;
    }

    NamedFile::open(Path::new("/home/detlev/pics").join(paths.last().unwrap().file_name())).ok()
}

#[get("/")]
fn website(state: State<Mutex<Data>>) -> Template {
    let data = state.lock().unwrap();
    Template::render("index", data.deref())
}

#[get("/update_telemetry")]
fn update_telemetry(state: State<Mutex<Data>>) -> Redirect {
    let mut data = state.lock().unwrap();
    data.commands.push(Command {
        duration: None,
        action: "telemetry".to_string(),
    });

    Redirect::to(uri!(website))
}

#[post("/request_watering", data = "<user_input>")]
fn request_watering(user_input: Form<WateringInput>, state: State<Mutex<Data>>) -> Redirect {
    let time_s: u8 = match user_input.time_s.parse() {
        Ok(t) => t,
        Err(_) => return Redirect::to(uri!(website)),
    };

    let mut data = state.lock().unwrap();
    data.commands.push(Command {
        duration: Some(time_s),
        action: "watering".to_string(),
    });

    Redirect::to(uri!(website))
}

fn main() {
    rocket::ignite()
        .mount(
            "/",
            routes![
                commands,
                telemetry,
                website,
                watering_status,
                last_pic,
                update_telemetry,
                request_watering
            ],
        )
        .manage(Mutex::new(Data {
            commands: vec![],
            telemetry: Telemetry {
                ..Default::default()
            },
            last_watering_time: None,
            last_watering_status: None,
            last_sent: "watering".to_string(),
        }))
        .attach(Template::fairing())
        .launch();
}
