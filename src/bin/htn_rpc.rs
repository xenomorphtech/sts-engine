use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use sts_engine::htn::HtnAgent;
use sts_engine::walk::{game_side, gameplay_mismatches, java_side_from_value, Side};
use sts_engine::{seed_from_string, Action, Character, Game, Screen, Unlocks};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

struct Session {
    seed: i64,
    game: Game,
    agent: HtnAgent,
    pending_decision: Option<Action>,
    steps: usize,
}

#[derive(Deserialize)]
struct CreateRequest {
    seed: Value,
    #[serde(default = "default_ascension")]
    ascension: i32,
    #[serde(default = "default_character")]
    character: String,
}

#[derive(Deserialize)]
struct StepRequest {
    action: Action,
}

#[derive(Deserialize)]
struct CompareRequest {
    observation: Value,
}

#[derive(Serialize)]
struct Observation {
    seed: i64,
    steps: usize,
    done: bool,
    screen: String,
    room: String,
    energy: i32,
    orbs: Vec<String>,
    hand_costs_for_turn: Vec<i16>,
    hand_upgraded: Vec<bool>,
    deck_upgraded: Vec<bool>,
    legal_actions: Vec<Action>,
    decision: Option<Action>,
    state: Side,
}

fn default_ascension() -> i32 {
    20
}

fn default_character() -> String {
    "DEFECT".to_string()
}

fn parse_character(raw: &str) -> Result<Character, String> {
    match raw.to_ascii_uppercase().as_str() {
        "DEFECT" => Ok(Character::Defect),
        "IRONCLAD" | "IRON_CLAD" => Ok(Character::Ironclad),
        "SILENT" | "THE_SILENT" => Ok(Character::Silent),
        "WATCHER" => Ok(Character::Watcher),
        _ => Err(format!("unsupported character {raw:?}")),
    }
}

fn parse_seed(value: &Value) -> Result<i64, String> {
    if let Some(seed) = value.as_i64() {
        return Ok(seed);
    }
    let raw = value
        .as_str()
        .ok_or_else(|| "seed must be an i64 or STS seed string".to_string())?;
    raw.parse::<i64>().or_else(|_| Ok(seed_from_string(raw)))
}

fn terminal(game: &Game) -> bool {
    game.done || game.player.hp <= 0 || game.screen == Screen::Terminal
}

fn refresh_decision(session: &mut Session) {
    session.pending_decision = if terminal(&session.game) {
        None
    } else {
        let decision = session.agent.decide(&session.game);
        (!matches!(decision, Action::Quit)).then_some(decision)
    };
}

fn observation(session: &Session) -> Observation {
    Observation {
        seed: session.seed,
        steps: session.steps,
        done: terminal(&session.game),
        screen: format!("{:?}", session.game.screen),
        room: format!("{:?}", session.game.current_room),
        energy: session.game.player.energy,
        orbs: session
            .game
            .player
            .orbs
            .iter()
            .map(|orb| format!("{:?}", orb.kind))
            .collect(),
        hand_costs_for_turn: session
            .game
            .player
            .hand
            .iter()
            .map(|card| card.cost_for_turn)
            .collect(),
        hand_upgraded: session
            .game
            .player
            .hand
            .iter()
            .map(|card| card.upgraded)
            .collect(),
        deck_upgraded: session
            .game
            .player
            .deck
            .iter()
            .map(|card| card.upgraded)
            .collect(),
        legal_actions: session.game.legal_actions(),
        decision: session.pending_decision.clone(),
        state: game_side(&session.game),
    }
}

fn json_response(status: u16, body: Value) -> Response<std::io::Cursor<Vec<u8>>> {
    let bytes = serde_json::to_vec(&body).expect("JSON response serialization failed");
    let header = Header::from_bytes("content-type", "application/json").expect("valid header");
    Response::from_data(bytes)
        .with_status_code(StatusCode(status))
        .with_header(header)
}

fn serialized_response<T: Serialize>(status: u16, body: &T) -> Response<std::io::Cursor<Vec<u8>>> {
    let bytes = serde_json::to_vec(body).expect("JSON response serialization failed");
    let header = Header::from_bytes("content-type", "application/json").expect("valid header");
    Response::from_data(bytes)
        .with_status_code(StatusCode(status))
        .with_header(header)
}

fn read_json(request: &mut Request) -> Result<Value, String> {
    let mut body = String::new();
    request
        .as_reader()
        .read_to_string(&mut body)
        .map_err(|error| format!("could not read request body: {error}"))?;
    serde_json::from_str(&body).map_err(|error| format!("invalid JSON body: {error}"))
}

fn session_route(url: &str) -> Option<(&str, Option<&str>)> {
    let tail = url.strip_prefix("/v1/sessions/")?;
    let mut pieces = tail.split('/');
    let id = pieces.next()?;
    let operation = pieces.next();
    (pieces.next().is_none()).then_some((id, operation))
}

fn handle_request(
    mut request: Request,
    sessions: &Arc<Mutex<HashMap<String, Session>>>,
    next_id: &Arc<AtomicU64>,
    unlocks: &Arc<Unlocks>,
) {
    let method = request.method().clone();
    let url = request.url().to_string();

    let response = if method == Method::Get && url == "/health" {
        json_response(200, json!({"status": "ok"}))
    } else if method == Method::Post && url == "/v1/sessions" {
        match read_json(&mut request)
            .and_then(|value| serde_json::from_value::<CreateRequest>(value).map_err(|error| error.to_string()))
            .and_then(|body| {
                let seed = parse_seed(&body.seed)?;
                let character = parse_character(&body.character)?;
                Ok((seed, character, body.ascension))
            }) {
            Ok((seed, character, ascension)) => {
                let game = Game::new(seed, character, ascension, (**unlocks).clone());
                let mut session = Session {
                    seed,
                    game,
                    agent: HtnAgent::new(),
                    pending_decision: None,
                    steps: 0,
                };
                refresh_decision(&mut session);
                let observation = observation(&session);
                let id = next_id.fetch_add(1, Ordering::Relaxed).to_string();
                sessions.lock().expect("session mutex poisoned").insert(id.clone(), session);
                json_response(201, json!({"session_id": id, "observation": observation}))
            }
            Err(message) => json_response(400, json!({"error": message})),
        }
    } else if let Some((id, operation)) = session_route(&url) {
        match (method, operation) {
            (Method::Get, None) => {
                let sessions = sessions.lock().expect("session mutex poisoned");
                match sessions.get(id) {
                    Some(session) => serialized_response(200, &observation(session)),
                    None => json_response(404, json!({"error": "unknown session"})),
                }
            }
            (Method::Delete, None) => {
                let removed = sessions.lock().expect("session mutex poisoned").remove(id);
                if removed.is_some() {
                    json_response(200, json!({"deleted": true}))
                } else {
                    json_response(404, json!({"error": "unknown session"}))
                }
            }
            (Method::Post, Some("step")) => match read_json(&mut request)
                .and_then(|value| serde_json::from_value::<StepRequest>(value).map_err(|error| error.to_string()))
            {
                Ok(body) => {
                    let mut sessions = sessions.lock().expect("session mutex poisoned");
                    match sessions.get_mut(id) {
                        None => json_response(404, json!({"error": "unknown session"})),
                        Some(session) if session.pending_decision.as_ref() != Some(&body.action) => json_response(
                            409,
                            json!({
                                "error": "action is not the current HTN decision",
                                "decision": session.pending_decision,
                                "action": body.action,
                            }),
                        ),
                        Some(session) => {
                            session.game.step(&body.action);
                            session.steps += 1;
                            refresh_decision(session);
                            serialized_response(200, &observation(session))
                        }
                    }
                }
                Err(message) => json_response(400, json!({"error": message})),
            },
            (Method::Post, Some("compare")) => match read_json(&mut request)
                .and_then(|value| serde_json::from_value::<CompareRequest>(value).map_err(|error| error.to_string()))
                .and_then(|body| java_side_from_value(&body.observation))
            {
                Ok(java) => {
                    let sessions = sessions.lock().expect("session mutex poisoned");
                    match sessions.get(id) {
                        None => json_response(404, json!({"error": "unknown session"})),
                        Some(session) => {
                            let rust = game_side(&session.game);
                            let mismatched = gameplay_mismatches(&rust, &java);
                            if mismatched.is_empty() {
                                json_response(200, json!({"matched": true, "mismatched": []}))
                            } else {
                                json_response(
                                    200,
                                    json!({
                                        "matched": false,
                                        "mismatched": mismatched,
                                        "rust": rust,
                                        "java": java,
                                    }),
                                )
                            }
                        }
                    }
                }
                Err(message) => json_response(400, json!({"error": message})),
            },
            _ => json_response(404, json!({"error": "unknown route"})),
        }
    } else {
        json_response(404, json!({"error": "unknown route"}))
    };

    let _ = request.respond(response);
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut host = "127.0.0.1".to_string();
    let mut port = 18082u16;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--host" => host = args.next().ok_or("--host requires a value")?,
            "--port" => {
                port = args
                    .next()
                    .ok_or("--port requires a value")?
                    .parse()
                    .map_err(|_| "invalid --port")?;
            }
            "--help" | "-h" => {
                println!("Usage: sts-htn-rpc [--host HOST] [--port PORT]");
                return Ok(());
            }
            _ => return Err(format!("unknown argument {arg:?}").into()),
        }
    }

    let address = format!("{host}:{port}");
    let server = Server::http(&address)?;
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let next_id = Arc::new(AtomicU64::new(1));
    let unlocks = Arc::new(Unlocks::fixture());
    eprintln!("STS HTN RPC listening on http://{address}");

    for request in server.incoming_requests() {
        let sessions = Arc::clone(&sessions);
        let next_id = Arc::clone(&next_id);
        let unlocks = Arc::clone(&unlocks);
        thread::spawn(move || handle_request(request, &sessions, &next_id, &unlocks));
    }
    Ok(())
}
