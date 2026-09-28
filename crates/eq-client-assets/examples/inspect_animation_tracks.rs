//! Lists local animation track names without exporting asset bytes.
use libeq::{
    pfs::PfsReader,
    wld::parser::{Track, WldDoc},
};
use std::{collections::BTreeMap, fs::File, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("pass a character archive")?;
    let model = std::env::args().nth(2).unwrap_or_else(|| "HUM".into());
    let mut archive = PfsReader::open(File::open(&path)?)?;
    let stem = Path::new(&path)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("archive name")?;
    let data = archive
        .get(&format!("{stem}.wld"))?
        .ok_or("missing world")?;
    let doc = WldDoc::parse(&data).map_err(|_| "invalid world")?;
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for track in doc.fragment_iter::<Track>() {
        if let Some(name) = doc.get_string(track.name_reference)
            && name.contains(&model)
        {
            groups
                .entry(name.chars().take(3).collect())
                .or_default()
                .push(name.to_owned());
        }
    }
    for (code, names) in groups {
        println!(
            "{code}: {} tracks, samples {:?}",
            names.len(),
            &names[..names.len().min(2)]
        );
    }
    Ok(())
}
