fn main() {
    println!("Hello, world!");
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

        assert_eq!(jar.len(), 1980);

        let mut classes = HashSet::new();
        for i in 0..jar.len() {
            let file = jar.by_index(i)?;
            if file.name().ends_with(".class") {
                let filename = file.sanitized_name();
                classes.insert(filename);
            }
        }

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
