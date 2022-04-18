use std::collections::HashMap;
use std::{fs, io, thread};
use std::path::Path;
use std::sync::{Arc, RwLock, RwLockWriteGuard};
use std::thread::JoinHandle;
use shared_memory::ShmemConf;
use machine::basic_globals::basic_globals;
use machine::command_module::command_provider;
use machine::global_provider::global_provider;
use runner::runner::JSRunner;
use runner::imports::Provider;

pub fn providers() -> Vec<Provider> {
    //All structs providing imports
    vec!(global_provider(), command_provider(), basic_globals())
}

fn main() {
    let processes: Arc<RwLock<HashMap<String, JoinHandle<()>>>> =
        Arc::new(RwLock::new(HashMap::new()));

    loop {
        println!("Testing!");
        let mut output = String::new();
        io::stdin().read_line(&mut output).unwrap();
        start_process(processes.write().unwrap(), output);
    }
}

fn start_process(mut map: RwLockWriteGuard<HashMap<String, JoinHandle<()>>>, output: String) {
    if !output.contains('\u{0000}') {
        if !map.contains_key(output.as_str()) {
            map.insert(
                output.clone(),
                thread::spawn(move || {
                    run(&output,
                        Option::None, vec![])
                }));
            return;
        }
        map.remove(output.as_str());
    } else {
        map.insert(
            String::from(output.split('\u{0000}').next().unwrap()),
            thread::spawn(move || {
                let split: Vec<&str> = output.split('\u{0000}').collect();
                run(&String::from(split[0]), Option::Some(String::from(split[1])),
                    split[2].split(",").collect())
            }));
    }
}

fn run(path: &String, memory_map: Option<String>, modules: Vec<&str>) {
    let params = v8::Isolate::create_params()
        .array_buffer_allocator(v8::new_default_allocator())
        .allow_atomics_wait(false)
        .heap_limits(0, 3 * 1024 * 1024);

    let mut found_providers = vec!();

    for provider in providers() {
        match provider.module {
            Some(module) => {
                if modules.contains(&module) {
                    found_providers.push(provider);
                }
            }
            _ => found_providers.push(provider)
        }
    }

    let memory;

    match memory_map {
        Some(path) => memory = Option::Some(ShmemConf::new().os_id(path).create().unwrap()),
        None => memory = Option::None
    }
    let mut module_sizes = HashMap::new();

    let mut i = 0;
    for module in modules {
        let split = module.find(':').unwrap();
        let size = module[split..].parse::<usize>().unwrap();

        module_sizes.insert(module[0..split].to_string(),
                            (i, size));
        i += size;
    }

    let mut runner = JSRunner::new(
        Option::None, params, found_providers, memory, module_sizes);

    let _result = runner.run(fs::read_to_string(Path::new(
        path)).unwrap().as_bytes());
}
