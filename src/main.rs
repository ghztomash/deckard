use std::collections::HashMap;
use std::collections::HashSet;

use clone_hunter::*;
mod cli;
mod files;

use files::FileEntry;

use colored::*;

use std::time::Instant;

#[tokio::main]
async fn main() {
    let args = cli::cli().get_matches();

    let target_dirs = match args.get_many::<String>("params") {
        Some(values) => values.map(|v| v.as_str()).collect::<Vec<&str>>(),
        None => vec!["."],
    };
    let depth = match args.get_one::<usize>("depth") {
        Some(v) => *v,
        None => usize::MAX,
    };
    let ignore_hidden = args.get_flag("ignore_hidden");

    let target_paths = collect_paths(target_dirs.clone());
    println!("Paths: {}", format!("{:?}", target_paths).yellow());

    let now = Instant::now();
    let files = visit_dirs(target_paths);

    let elapsed = now.elapsed();
    // let mut vec_files = Vec::from_iter(files.values());
    let mut vec_files = Vec::with_capacity(files.capacity());
    println!(
        "Indexed {} files in {}",
        files.capacity().to_string().green(),
        format!("{:.2?}", elapsed).blue()
    );

    let now = Instant::now();
    for (_, mut f) in files.clone() {
        f.process();
        vec_files.push(f);
    }
    let elapsed = now.elapsed();
    println!(
        "Processed {} files in {}",
        files.capacity().to_string().green(),
        format!("{:.2?}", elapsed).blue()
    );

    let mut file_matches: HashMap<String, HashSet<String>> = HashMap::new();

    let now = Instant::now();
    for i in 0..vec_files.len() {
        for j in i + 1..vec_files.len() {
            let f = &vec_files[i];
            let ff = &vec_files[j];
            let matching = f.compare(ff);
            if matching {
                match file_matches.get_mut(&f.id) {
                    Some(ref mut v) => {
                        v.insert(ff.id.clone());
                    }
                    None => {
                        file_matches.insert(f.id.clone(), HashSet::from([ff.id.clone()]));
                    }
                };
                match file_matches.get_mut(&ff.id) {
                    Some(ref mut v) => {
                        v.insert(f.id.clone());
                    }
                    None => {
                        file_matches.insert(ff.id.clone(), HashSet::from([f.id.clone()]));
                    }
                };
                //f.matches(ff.id);
                //ff.matches(f.id);
            }
        }
    }

    let elapsed = now.elapsed();
    println!(
        "Found {} matches in {}",
        file_matches.len().to_string().green(),
        format!("{:.2?}", elapsed).blue()
    );

    println!("\nMatches:");
    for (file, file_copies) in file_matches {
        let name = files.get(&file).unwrap().name.clone();
        let mut match_names = Vec::new();

        for fc in file_copies {
            match_names.push(files.get(&fc).unwrap().name.clone());
        }

        println!("{} matches {:?}", name, match_names);
    }
}
