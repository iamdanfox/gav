use core::fmt::{Debug, Write};
use std::collections::BTreeMap;
use std::env::args;
use std::ffi::OsString;
use std::fmt::{Display, Error, Formatter};
use std::fs::File;
use std::io::Cursor;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use semver::Version;
use walkdir::{DirEntry, WalkDir};
use zip::ZipArchive;

fn main() -> Result<(), std::io::Error> {
    let args: Vec<String> = args().collect();
    let first_arg = &args.get(1).expect("Usage: gav /path/to/jars");

    println!("Indexing {}", first_arg);
    let before = Instant::now();
    let mut classes: BTreeMap<String, Vec<OsString>> = BTreeMap::new();
    for walkdir in WalkDir::new(first_arg)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(is_jar)
    {
        let file = File::open(walkdir.path())?;

        let mut jar = ZipArchive::new(file)?;

        for i in 0..jar.len() {
            let jar_entry = jar.by_index(i)?;

            if jar_entry.name().contains('$') {
                // TODO(dfox): don't keep anonymous inner classes (e.g. CacheKey$1)
                continue;
            }

            if !jar_entry.name().ends_with(".class") {
                continue;
            }

            let class = jar_entry.sanitized_name();
            classes
                .entry(class.to_str().unwrap().to_string())
                .or_default()
                .push((*walkdir.path().as_os_str()).to_os_string());
        }
    }
    let after = Instant::now();
    let duration = after.duration_since(before);
    println!("Indexed {:?} classes in {:?}", classes.len(), duration);

    let keys: Vec<String> = classes
        .keys()
        .map(|s| s.to_owned().replace("/", ".").replace(".class", ""))
        .collect();
    let classnames_for_skim = keys.join("\n");

    let options = skim::SkimOptionsBuilder::default()
        .prompt(Some("class:"))
        .tiebreak(Some("score,end,-begin,index".to_string()))
        .delimiter(Some("."))
        .build()
        .unwrap();

    let vec = skim::Skim::run_with(&options, Some(Box::new(Cursor::new(classnames_for_skim))))
        .map(|out| out.selected_items)
        .unwrap_or_else(|| Vec::new());

    let item = vec.first().unwrap();
    println!("Selected {}: {}", item.get_index(), item.get_output_text());

    let jars = classes
        .iter()
        .nth(item.get_index())
        .expect("index should be a hit")
        .1;

    dbg!(jars);

    Ok(())
}

fn is_jar(e: &DirEntry) -> bool {
    e.file_name()
        .to_str()
        .map(|s| s.ends_with(".jar") && !s.ends_with("-sources.jar"))
        .unwrap_or(false)
}

/// We want to prioritise crawling the newest jar of every coordinate first, but they might not
/// all be semver compliant!
fn semver_greatest_first(vec: Vec<String>) -> (Vec<String>, Vec<String>) {
    let (semver, non_semver): (Vec<String>, Vec<String>) = vec
        .into_iter()
        .partition(|string| Version::parse(&string).is_ok());

    let mut parsed: Vec<Version> = semver.iter().map(|v| Version::parse(v).unwrap()).collect();
    parsed.sort();

    return (
        parsed.iter().map(|v| format!("{}", v)).rev().collect(),
        non_semver,
    );
}

#[derive(Debug)]
struct GroupArtifact {
    group: String,
    name: String,
    path: String,
}

struct GroupArtifactVersion {
    group: String,
    name: String,
    version: String,
}

impl Display for GroupArtifactVersion {
    fn fmt(&self, f: &mut Formatter) -> Result<(), Error> {
        f.write_str(&self.group)?;
        f.write_char(':')?;
        f.write_str(&self.name)?;
        f.write_char(':')?;
        f.write_str(&self.version)?;
        Ok(())
    }
}

impl Debug for GroupArtifactVersion {
    fn fmt(&self, f: &mut Formatter) -> Result<(), Error> {
        Display::fmt(self, f)
    }
}

struct GradleJarCache {
    root: PathBuf,
}

impl GradleJarCache {
    pub fn find_jars(&self) -> Vec<GroupArtifact> {
        return WalkDir::new(&self.root)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|d| d.file_type().is_dir())
            .filter(|d| d.depth() == 2)
            .map(|d| {
                let path = d.path();
                let components = path.components();
                let count: Vec<Component> = components.rev().take(2).collect();
                GroupArtifact {
                    group: count[1].as_os_str().to_str().unwrap().to_string(),
                    name: count[0].as_os_str().to_str().unwrap().to_string(),
                    path: path.to_str().unwrap().to_string(),
                }
            })
            .collect();
    }

    pub fn path(&self, gav: GroupArtifactVersion) -> PathBuf {
        let mut buf = self.root.clone();
        buf.push(gav.group);
        buf.push(gav.name);
        buf.push(gav.version);

        let jar: Option<DirEntry> = WalkDir::new(buf)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(is_jar)
            .nth(0);

        jar.expect("GAV did not contain a .jar").into_path()
    }

    pub fn find_jars_latest_first(&self) -> Vec<GroupArtifactVersion> {
        let mut vec1: Vec<GroupArtifactVersion> = Vec::new();
        //        let mut vec2: Vec<GroupArtifactVersion> = Vec::new();

        self.find_jars().iter().for_each(|ga| {
            let versions: Vec<String> = WalkDir::new(&ga.path)
                .max_depth(1)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|d| d.file_type().is_dir())
                .filter(|d| d.depth() == 1)
                .map(|d| d.file_name().to_str().unwrap().to_string())
                .collect();

            let (orderable, non_orderable) = semver_greatest_first(versions);

            let mut orderable_gavs: Vec<GroupArtifactVersion> = orderable
                .into_iter()
                .map(|v| GroupArtifactVersion {
                    group: ga.group.clone(),
                    name: ga.name.clone(),
                    version: v,
                })
                .collect();

            if let Some(head) = orderable_gavs.pop() {
                vec1.push(head);
            }

            //            let non_orderable_gavs = non_orderable.into_iter().map(|v| GroupArtifactVersion {
            //                group: ga.group.clone(),
            //                name: ga.name.clone(),
            //                version: v,
            //            });
            //
            //            let mut both: Vec<GroupArtifactVersion> =
            //                orderable_gavs.chain(non_orderable_gavs).collect();

            //            if let Some(head) = both.pop() {
            //                vec1.push(head);
            //                vec2.extend(both);
            //            }
        });

        //        vec1.extend(vec2);
        vec1
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashSet;
    use std::fs::File;
    use std::path::PathBuf;

    use crate::GroupArtifactVersion;
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

    #[test]
    fn do_stuff() {
        let cache = super::GradleJarCache {
            root: PathBuf::from("/Users/dfox/.gradle/caches/modules-2/files-2.1/"),
        };
        cache.find_jars_latest_first();
    }

    // walkdir can find every single file in the gradle cache in roughly 1 second, but the jar parsing is expensive
    // by picking only

    #[test]
    fn how_fast_can_we_crawl() -> Result<(), std::io::Error> {
        let cache = super::GradleJarCache {
            root: PathBuf::from("/Users/dfox/.gradle/caches/modules-2/files-2.1/"),
        };

        dbg!(cache.find_jars());
        Ok(())
    }

    #[test]
    fn sort_newest_first() {
        let vec = vec![
            "1.0.0".to_string(),
            "2.0.0".to_string(),
            "3.0.0".to_string(),
            "5.3.4.Final".to_string(),
        ];

        let (orderable, non_orderable) = super::semver_greatest_first(vec);

        assert_eq!(orderable, vec!["3.0.0", "2.0.0", "1.0.0"]);
        assert_eq!(non_orderable, vec!["5.3.4.Final"]);
    }

    #[test]
    fn find_jars_latest_first() {
        let cache = super::GradleJarCache {
            root: PathBuf::from("/Users/dfox/.gradle/caches/modules-2/files-2.1/"),
        };
        dbg!(cache.find_jars_latest_first());
    }

    #[test]
    fn find_single_path() {
        let cache = super::GradleJarCache {
            root: PathBuf::from("/Users/dfox/.gradle/caches/modules-2/files-2.1/"),
        };
        let buf = cache.path(GroupArtifactVersion {
            group: "io.searchbox".to_string(),
            name: "jest".to_string(),
            version: "0.1.7".to_string(),
        });
        println!("{:?}", buf);
    }
}
