use std::collections::HashMap;
use std::collections::HashSet;

use clone_hunter::*;
mod cli;
mod files;

use files::FileEntry;

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
    println!("Paths: {:?}", target_paths);
    println!("Files:");
    let files = visit_dirs(target_paths);
    // let mut vec_files = Vec::from_iter(files.values());
    let mut vec_files = Vec::with_capacity(files.capacity());

    for (_, mut f) in files.clone() {
        f.process();
        vec_files.push(f);
    }

    let mut file_matches: HashMap<String, HashSet<String>> = HashMap::new();

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
