use std::collections::HashMap;
use std::env::args;
use std::ffi::OsString;
use std::fs::File;
use std::time::Instant;

use walkdir::{DirEntry, WalkDir};
use zip::ZipArchive;

fn main() -> Result<(), std::io::Error> {
    let args: Vec<String> = args().collect();
    let first = &args.get(1).expect("Usage: gav /path/to/jars");

    let is_jar = |e: &DirEntry| -> bool {
        e.file_name()
            .to_str()
            .map(|s| s.ends_with(".jar") && !s.ends_with("-sources.jar"))
            .unwrap_or(false)
    };

    let before = Instant::now();
    let mut classes: HashMap<OsString, Vec<OsString>> = HashMap::new();

    for walkdir in WalkDir::new(first)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(is_jar)
    {
        let file = File::open(walkdir.path())?;

        let mut jar = ZipArchive::new(file)?;

        for i in 0..jar.len() {
            let jar_entry = jar.by_index(i)?;
            if jar_entry.name().ends_with(".class") {
                let class = jar_entry.sanitized_name();
                classes
                    .entry(class.into_os_string())
                    .or_default()
                    .push((*walkdir.path().as_os_str()).to_os_string());
            }
        }

        println!("{:?}", walkdir.path());
    }

    let after = Instant::now();
    let duration = after.duration_since(before);
    let i1 = classes.len();

    println!("Indexed {:?} classes in {:?}", i1, duration);

    dbg!(&classes);
    Ok(())
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
