use std::collections::HashMap;
use std::env::args;
use std::ffi::OsString;
use std::fs::File;
use std::io::stdin;
use std::time::Instant;

use walkdir::{DirEntry, WalkDir};
use zip::ZipArchive;

fn main() -> Result<(), std::io::Error> {
    let args: Vec<String> = args().collect();
    let first_arg = &args.get(1).expect("Usage: gav /path/to/jars");

    println!("Indexing {}", first_arg);
    let before = Instant::now();
    let mut classes: HashMap<String, Vec<OsString>> = HashMap::new();
    for walkdir in WalkDir::new(first_arg)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(is_jar)
    {
        let file = File::open(walkdir.path())?;

        let mut jar = ZipArchive::new(file)?;

        for i in 0..jar.len() {
            let jar_entry = jar.by_index(i)?;
            if jar_entry.name().ends_with(".class") {
                // TODO(dfox): don't keep anonymous inner classes (e.g. CacheKey$1)
                let class = jar_entry.sanitized_name();
                classes
                    .entry(class.to_str().unwrap().to_string())
                    .or_default()
                    .push((*walkdir.path().as_os_str()).to_os_string());
            }
        }
    }
    let after = Instant::now();
    let duration = after.duration_since(before);
    println!("Indexed {:?} classes in {:?}", classes.len(), duration);

    dbg!(&classes.iter().nth(1).unwrap());

    loop {
        println!("Enter a search term, or Ctrl-C to exist:");

        let mut user_input = String::new();
        stdin().read_line(&mut user_input)?;

        let search_term = user_input.trim().to_lowercase();

        classes
            .iter()
            .filter(|(key, _)| key.to_lowercase().contains(&search_term))
            .take(4)
            .for_each(|(key, gavs)| {
                println!("\t{} {}", key, gavs.len());

                for gav in gavs {
                    println!("\t\t{:?}", gav);
                }
            });
    }

    Ok(())
}

fn is_jar(e: &DirEntry) -> bool {
    e.file_name()
        .to_str()
        .map(|s| s.ends_with(".jar") && !s.ends_with("-sources.jar"))
        .unwrap_or(false)
}

#[cfg(test)]
mod test {
    use std::collections::HashSet;
    use std::fs::File;
    use std::path::PathBuf;

    use zip::ZipArchive;

    #[test]
    fn parse_jar() -> Result<(), std::io::Error> {
        let file = File::open("./resources/guava-27.1-jre.jar")?;
        let mut jar = ZipArchive::new(file)?;

        let mut classes = HashSet::new();
        for i in 0..jar.len() {
            let file = jar.by_index(i)?;
            if file.name().ends_with(".class") {
                let filename = file.sanitized_name();
                classes.insert(filename);
            }
        }

        assert_eq!(jar.len(), 1980);
        assert!(classes.contains(&PathBuf::from(
            "com/google/common/collect/ImmutableList.class"
        )));
        assert!(classes.contains(&PathBuf::from(
            "com/google/common/collect/ImmutableList$Builder.class"
        )));
        assert_eq!(classes.len(), 1950);
        Ok(())
    }
}
