use core::fmt::{Debug, Write};
use std::collections::BTreeMap;
use std::fmt::{Display, Error, Formatter};
use std::fs::{File, OpenOptions};
use std::io::Cursor;
use std::path::{Component, PathBuf};
use std::time::Instant;

use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use semver::Version;
use serde::de::Visitor;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, thread};
use walkdir::{DirEntry, WalkDir};
use zip::ZipArchive;

fn main() -> Result<(), std::io::Error> {
    let before = Instant::now();
    let cache = GradleJarCache {
        root: PathBuf::from("/Users/dfox/.gradle/caches/modules-2/files-2.1/"),
    };

    let gavs = cache.find_jars_latest_first();
    eprintln!(
        "{} {}",
        "[1/2]".white().dimmed(),
        format!(
            "Found {:?} gavs in {:?}",
            gavs.len(),
            Instant::now().duration_since(before)
        )
    );

    let pb = ProgressBar::new(gavs.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.white/cyan} {pos:>7}/{len:7} {msg}")
            .progress_chars("##-"),
    );

    let entries: Vec<(String, GroupArtifactVersion)> = gavs
        .par_iter()
        .flat_map(|gav| {
            pb.set_message(&format!("{}", gav));
            pb.inc(1);

            let maybe_jar_path = cache.jar_for_path(&gav);
            if maybe_jar_path.is_none() {
                return Vec::new();
            }
            let jar_path = maybe_jar_path.unwrap();

            let jar_file = File::open(&jar_path).unwrap();
            let mut jar = ZipArchive::new(jar_file).unwrap();

            let vec: Vec<(String, GroupArtifactVersion)> = (0..jar.len())
                .map(|i| {
                    let jar_entry = jar.by_index(i).unwrap();

                    if jar_entry.name().contains('$') {
                        // TODO(dfox): don't keep anonymous inner classes (e.g. CacheKey$1)
                        return None;
                    }

                    if !jar_entry.name().ends_with(".class") {
                        return None;
                    }

                    let class = jar_entry.sanitized_name();

                    Some((class.to_str().unwrap().to_string(), gav.clone()))
                })
                .filter_map(|pair| pair)
                .collect();
            vec
        })
        .collect();

    let mut classes: BTreeMap<String, Vec<GroupArtifactVersion>> = BTreeMap::new();
    for (class, jar) in entries {
        classes.entry(class).or_default().push(jar);
    }

    let persisted = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open("/Users/dfox/Desktop/foo.json")
        .unwrap();
    serde_json::to_writer_pretty(persisted, &classes).unwrap();

    pb.finish_and_clear();

    let duration = Instant::now().duration_since(before);
    eprintln!(
        "{} {}",
        "[2/2]".white().dimmed(),
        format!("Indexed {:?} classes in {:?}", classes.len(), duration)
    );

    let options = skim::SkimOptionsBuilder::default()
        .tiebreak(Some("score,end,-begin,index".to_string()))
        .delimiter(Some("."))
        .build()
        .unwrap();

    let keys: Vec<String> = classes
        .keys()
        .map(|s| s.to_owned().replace("/", ".").replace(".class", ""))
        .collect();
    let classnames_for_skim = keys.join("\n");
    let vec = skim::Skim::run_with(&options, Some(Box::new(Cursor::new(classnames_for_skim))))
        .map(|out| out.selected_items)
        .unwrap_or_else(|| Vec::new());

    let selected = vec.first().unwrap();
    classes
        .iter()
        .nth(selected.get_index())
        .expect("index should be a hit")
        .1
        .iter()
        .for_each(|gav| println!("{}", format!("{}", gav).white()));

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

#[derive(Debug, Ord, Eq, PartialOrd, PartialEq)]
struct GroupArtifact {
    group: String,
    name: String,
    path: String,
}

#[derive(Clone, Ord, Eq, PartialOrd, PartialEq)]
struct GroupArtifactVersion {
    group: String,
    name: String,
    version: String,
}

impl Serialize for GroupArtifactVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("{}", self))
    }
}

impl<'de> Deserialize<'de> for GroupArtifactVersion {
    fn deserialize<D>(deserializer: D) -> Result<GroupArtifactVersion, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(GavVisitor)
    }
}

struct GavVisitor;

impl<'de> Visitor<'de> for GavVisitor {
    type Value = GroupArtifactVersion;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string, e.g. \"com.foo:bar:1.2.3\"")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let split: Vec<&str> = v.split(":").collect();

        Ok(GroupArtifactVersion {
            group: split[0].to_string(),
            name: split[1].to_string(),
            version: split[2].to_string(),
        })
    }
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

    /// returns empty if the gav didn't contain a .jar (e.g. perhaps it just contained a pom)
    pub fn jar_for_path(&self, gav: &GroupArtifactVersion) -> Option<PathBuf> {
        let mut buf = self.root.clone();
        buf.push(&gav.group);
        buf.push(&gav.name);
        buf.push(&gav.version);

        WalkDir::new(&buf)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(is_jar)
            .nth(0)
            .map(|d| d.into_path())
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

            let (orderable, _non_orderable) = semver_greatest_first(versions);

            let orderable_gavs = orderable
                .into_iter()
                .map(|v| GroupArtifactVersion {
                    group: ga.group.clone(),
                    name: ga.name.clone(),
                    version: v,
                })
                .nth(0);

            //            let non_orderable_gavs = non_orderable.into_iter().map(|v| GroupArtifactVersion {
            //                group: ga.group.clone(),
            //                name: ga.name.clone(),
            //                version: v,
            //            });

            //            let mut both: Vec<GroupArtifactVersion> =
            //                orderable_gavs.chain(non_orderable_gavs).collect();

            if let Some(head) = orderable_gavs {
                vec1.push(head);
            }

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

    use zip::ZipArchive;

    use crate::GroupArtifactVersion;

    #[test]
    fn parse_jar() -> Result<(), std::io::Error> {
        let file =
            File::open("resources/com.google.guava/guava/27.1-jre/somehash/guava-27.1-jre.jar")?;
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
            root: PathBuf::from("resources"),
        };
        cache.find_jars_latest_first();
    }

    // walkdir can find every single file in the gradle cache in roughly 1 second, but the jar parsing is expensive
    // by picking only

    #[test]
    fn how_fast_can_we_crawl() -> Result<(), std::io::Error> {
        let cache = super::GradleJarCache {
            root: PathBuf::from("resources"),
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
            root: PathBuf::from("resources"),
        };
        dbg!(cache.find_jars_latest_first());
    }

    #[test]
    fn find_single_path() {
        let cache = super::GradleJarCache {
            root: PathBuf::from("resources"),
        };
        let buf = cache.jar_for_path(&GroupArtifactVersion {
            group: "com.google.guava".to_string(),
            name: "guava".to_string(),
            version: "27.1-jre".to_string(),
        });
        println!("{:?}", buf);
    }

    #[test]
    fn serde_for_gavs() -> Result<(), std::io::Error> {
        let version = GroupArtifactVersion {
            group: "com.google.guava".to_string(),
            name: "guava".to_string(),
            version: "23.6-jre".to_string(),
        };
        let result = serde_json::to_string(&version)?;

        assert_eq!(result, "\"com.google.guava:guava:23.6-jre\"");

        let deserialized: GroupArtifactVersion =
            serde_json::from_str("\"com.google.guava:guava:23.6-jre\"")?;

        assert_eq!(deserialized, version);

        Ok(())
    }
}
