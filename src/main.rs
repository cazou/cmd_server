#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use]
extern crate rocket;
extern crate core;

use chrono::{DateTime, Utc};
use home::home_dir;
use md5;
use rand::distributions::{Alphanumeric, DistString};
use rand::{thread_rng, Rng};
use rocket::http::{Cookie, Cookies};
use rocket::request::Form;
use rocket::response::{NamedFile, Redirect};
use rocket::State;
use rocket_contrib::json;
use rocket_contrib::json::{Json, JsonValue};
use rocket_contrib::serve::StaticFiles;
use rocket_contrib::templates::Template;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs;
use std::fs::{DirEntry, File};
use std::io::{self, BufRead};
use std::ops::Deref;
use std::path::Path;
use std::sync::Mutex;

#[derive(FromForm)]
struct WateringInput {
    time_s: String,
}

#[derive(FromForm)]
struct LoginCred {
    username: String,
    password: String,
}

#[derive(Serialize, Deserialize)]
struct Login {
    username: String,
    key: String,
}

//TODO:
// * TLS

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
    username: String,
    password: String,
    active_logins: Vec<Login>,
}

fn loggedin(cookies: &mut Cookies, data: &Data) -> bool {
    if let Some(k) = cookies.get_private("key") {
        // Try to find the key in logins
        return data.active_logins.iter().any(|l| l.key == k.value());
    }

    false
}

#[post("/login", data = "<creds>")]
fn login(creds: Form<LoginCred>, mut cookies: Cookies, state: State<Mutex<Data>>) -> Redirect {
    if let Some(_) = cookies.get_private("key") {
        return Redirect::to(uri!(website));
    }

    let mut data = state.lock().unwrap();

    if creds.password != data.password || creds.username != data.username {
        Redirect::to(uri!(website));
    }

    let salt = Alphanumeric.sample_string(&mut rand::thread_rng(), 16);
    let key = String::from_utf8_lossy(
        &md5::compute(format!("{}{}{}", salt, creds.username, creds.password)).0,
    )
    .to_string();

    let login = Login {
        username: creds.username.clone(),
        key: key.clone(),
    };

    data.active_logins.push(login);

    cookies.add_private(Cookie::new("key", key));

    return Redirect::to(uri!(website));
}

#[get("/commands")]
fn commands(state: State<Mutex<Data>>) -> Json<Vec<Command>> {
    let mut data = state.lock().unwrap();
    let ret = Json(data.commands.clone());
    data.commands.clear();

    ret
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
fn last_pic(mut cookies: Cookies, state: State<Mutex<Data>>) -> Option<NamedFile> {
    if !loggedin(&mut cookies, &state.lock().unwrap()) {
        return None;
    }

    let paths = match fs::read_dir("/home/admin/pics/") {
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
fn website(mut cookies: Cookies, state: State<Mutex<Data>>) -> Template {
    let data = state.lock().unwrap();
    if loggedin(&mut cookies, &data) {
        Template::render("index", data.deref())
    } else {
        Template::render("login", data.deref())
    }
}

#[get("/update_telemetry")]
fn update_telemetry(mut cookies: Cookies, state: State<Mutex<Data>>) -> Redirect {
    let mut data = state.lock().unwrap();
    if loggedin(&mut cookies, &data) {
        data.commands.push(Command {
            duration: None,
            action: "telemetry".to_string(),
        });
    }

    Redirect::to(uri!(website))
}

#[post("/request_watering", data = "<user_input>")]
fn request_watering(
    mut cookies: Cookies,
    user_input: Form<WateringInput>,
    state: State<Mutex<Data>>,
) -> Redirect {
    let time_s: u8 = match user_input.time_s.parse() {
        Ok(t) => t,
        Err(_) => return Redirect::to(uri!(website)),
    };

    let mut data = state.lock().unwrap();

    if loggedin(&mut cookies, &data) {
        data.commands.push(Command {
            duration: Some(time_s),
            action: "watering".to_string(),
        });
    }

    Redirect::to(uri!(website))
}

fn read_lines<P>(filename: P) -> io::Result<io::Lines<io::BufReader<File>>>
where
    P: AsRef<Path>,
{
    let file = File::open(filename)?;
    Ok(io::BufReader::new(file).lines())
}

fn main() {
    let (username, password) =
        if let Ok(mut lines) = read_lines(home_dir().unwrap().join("cmd_server.cfg")) {
            (
                lines.next().unwrap_or(Ok("admin".to_string())).unwrap(),
                lines.next().unwrap_or(Ok("admin".to_string())).unwrap(),
            )
        } else {
            ("admin".to_string(), "admin".to_string())
        };

    let data = Data {
        commands: vec![],
        telemetry: Telemetry {
            ..Default::default()
        },
        last_watering_time: None,
        last_watering_status: None,
        last_sent: "watering".to_string(),
        active_logins: vec![],
        username,
        password,
    };

    let routes = routes![
        commands,
        telemetry,
        website,
        login,
        watering_status,
        last_pic,
        update_telemetry,
        request_watering,
    ];

    rocket::ignite()
        .mount("/", routes)
        .mount("/css", StaticFiles::from("css"))
        .manage(Mutex::new(data))
        .attach(Template::fairing())
        .launch();
}
