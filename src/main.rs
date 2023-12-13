use git2::Index;

fn main() -> Result<(), anyhow::Error> {
    create_tree()?;

    Ok(())
}

fn create_tree() -> Result<(), anyhow::Error> {
    let mut index = Index::new()?;
    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    let oid = index.write_tree()?;
    println!("{:?}", oid);

    Ok(())
}
