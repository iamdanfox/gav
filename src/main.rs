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

        let mut filenames = HashSet::new();
        for i in 0..jar.len() {
            let file = jar.by_index(i)?;
            let filename = file.sanitized_name();
            filenames.insert(filename);
        }

        assert!(filenames.contains(&PathBuf::from(
            "com/google/common/collect/ImmutableList.class"
        )));
        assert!(filenames.contains(&PathBuf::from(
            "com/google/common/collect/ImmutableList$Builder.class"
        )));
        Ok(())
    }
}
