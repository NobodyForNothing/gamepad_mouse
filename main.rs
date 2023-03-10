use std::io::prelude::*;
use std::io::{BufReader, Cursor};
use std::fs::File;
use std::sync::{Arc, Mutex};
use std::{thread};
use std::time::{SystemTime, Duration};
use byteorder::{BigEndian, ReadBytesExt};
use enigo::*;

static MOVE_DIVISOR: f64 = 1638.4; // = (x/32768)*20 // smaller -> faster
static CLAMP_THRESHOLD: f64 = 0.04;
static SCROLL_MUTLIPLIER: f64 = 1.5;
static CONTROLLER_NAMEPARTS: [&'static str; 2] = ["gamepad", "controller"];

struct State {
    running: bool,
    x: i16,
    y: i16,
    rx: f64,
    ry: f64,
    z: i16,
    rz: i16,
}


fn get_gamepad_handler() -> String {
    // 1. open device file
    // 2. read 2nd line and then evry 10 lines further to get device name. find a name containing 'gamepad' (case insensitive)
    // 3. 4 lines below are the Handlers. After the equal sign are space speperated handlers; we want the one containing event
    // 4. return the handlercontaining event
    let f = File::open("/proc/bus/input/devices").unwrap();
    let reader = BufReader::new(f);

    let mut lines = reader.lines();
    
    let mut found_controller = false;
    while !found_controller {
        let line = lines.next();
        if line.is_none() {
            println!("No controler found! This could be caused by the controllers name not containing any of the words:");
            println!("{:?}", CONTROLLER_NAMEPARTS);
            std::process::exit(1);
        }
        let controller_name = line.unwrap().unwrap().to_lowercase();
        for name in CONTROLLER_NAMEPARTS {
            if controller_name.contains(name) {
                found_controller = true;
            }
        }
    }

    let handlers_line = lines.nth(3).unwrap().unwrap();
    let mut handlers = handlers_line.split("=").nth(1).unwrap().split(" ");
    
    let y = handlers.find(|x| x.contains("event")).unwrap().to_string();
    y.clone()
}


fn main() -> std::io::Result<()> {
    let mut file_location = "/dev/input/".to_owned();
    let eventfile_name = get_gamepad_handler();
    file_location.push_str(&eventfile_name);
    println!("{}", file_location);
    let f = File::open(file_location)?;
    let mut reader = BufReader::new(f);


    let state = Arc::new(Mutex::new(State {
        running: true,
        x: 0,
        y: 0,
        rx: 0.,
        ry: 0.,
        z: 0,
        rz: 0,
    }));


    let poll_state = Arc::clone(&state);
    let poll = thread::spawn(move || { // some parts that logically don't belong here are here for performance resons
        let mut enigo = Enigo::new();
        loop {
            // read from file
            let mut buf = vec![];
            let mut chunk = (&mut reader).take(24);
            chunk.read_to_end(&mut buf).expect("Didn't read enough");

            let mut code:u16 = buf[19].into();
            code *= 256;
            code += buf[18] as u16;

            //println!("{:?}", buf);

            let mut state = poll_state.lock().unwrap();
            match code {
                0 => { // ABS_X
                    let mut tmp = Cursor::new(buf[21..23].to_vec());
                    let value = tmp.read_i16::<BigEndian>().unwrap();
                    state.x = value;
                }
                1 => { // ABS_Y
                    let mut tmp = Cursor::new(buf[21..23].to_vec());
                    let value = tmp.read_i16::<BigEndian>().unwrap();
                    state.y = value;
                }
                3 => { // ABS_RX
                    let mut tmp = Cursor::new(buf[21..23].to_vec());
                    let value = tmp.read_i16::<BigEndian>().unwrap();
                    let mut value = (value as f64)/MOVE_DIVISOR;  // todo: move logic
                    if value.abs() < CLAMP_THRESHOLD { // prevent mouse movent despite unmoved joystic
                        value = 0.;
                    }
                    state.rx = value as f64;
                }
                4 => { // ABS_RY
                    let mut tmp = Cursor::new(buf[21..23].to_vec());
                    let value = tmp.read_i16::<BigEndian>().unwrap();
                    let mut value = (value as f64)/MOVE_DIVISOR;
                    if value.abs() < CLAMP_THRESHOLD { // prevent mouse movent despite unmoved joystic
                        value = 0.;
                    }
                    state.ry = value;
                }
                2 => { // ABS_Z
                    state.z = buf[20].into(); 
                }
                5 => { // ABS_RZ
                    state.rz = buf[20].into(); 
                }
                16 => {
                    if buf[20] == 1 {
                        enigo.key_click(Key::RightArrow);
                    } else if buf[20] == 255 {
                        enigo.key_click(Key::LeftArrow);
                    }
                }
                17 => {
                    if buf[20] == 1 {
                        enigo.key_click(Key::DownArrow);
                    } else if buf[20] == 255 {
                        enigo.key_click(Key::UpArrow);
                    }
                }
                304 => { // button A
                    if buf[20] == 1 {
                        enigo.key_click(Key::Return);
                    }
                }
                310 => { // BTN_TL
                    if buf[20] == 1 {
                        enigo.mouse_click(MouseButton::Back);
                    }
                }
                311 => { // BTN_TR
                    if buf[20] == 1 {
                        enigo.mouse_click(MouseButton::Foreward);
                    }
                }   
                314 => { // BTN_SELECT
                    state.running = false;
                }
                315 => { // start
                    if buf[20] == 1 {
                        enigo.key_click(Key::Meta);
                    }
                }
                _ => {
                    println!("unsupported keycode: {}", code)
                }
            }
            if !state.running { 
                break;
            }
        }
    });


    let update_state = Arc::clone(&state);
    let update = thread::spawn(move || {
        let mut last_ex = SystemTime::now();
        let mut enigo = Enigo::new();
        
        loop {
            if last_ex.elapsed().unwrap().as_millis() > 10 {
                last_ex = SystemTime::now();

                let state = update_state.lock().unwrap();
                if !state.running { 
                    break;
                }

                // move mouse
                enigo.mouse_move_relative((state.rx) as i32, 0);
                enigo.mouse_move_relative(0, (state.ry) as i32);

                // mouse buttons
                if state.rz > 128 {
                    enigo.mouse_down(MouseButton::Right);
                } else if state.rz > 0 {
                    enigo.mouse_up(MouseButton::Right);
                }
                if state.z > 128 {
                    enigo.mouse_down(MouseButton::Left);
                } else if state.z > 0 {
                    enigo.mouse_up(MouseButton::Left);
                }    

                // scroll
                let y = (state.y as f64)/32768.;
                enigo.mouse_scroll_y((y*SCROLL_MUTLIPLIER) as i32);
            } else {
                thread::sleep(Duration::from_millis(1));
            }
        }
    });


    poll.join().unwrap();
    update.join().unwrap();
        
    Ok(())
}