use ms_package::authoring::*;
use std::{io::Cursor, path::PathBuf};
struct Sink(PathBuf);
impl InstallerMediaSink for Sink {
    type Writer=std::fs::File;
    fn create(&mut self,name:&str)->std::io::Result<Self::Writer>{let p=self.0.join(name);std::fs::create_dir_all(p.parent().unwrap())?;std::fs::File::create(p)}
    fn finish(&mut self,_:&str,w:Self::Writer)->std::io::Result<()>{w.sync_all()}
}
fn guid(cell:u32,kind:u32)->String{format!("{{C0A90009-{cell:04X}-4B31-ABCD-{kind:012X}}}")}
struct Resolver(PathBuf);
impl ms_package::MediaResolver for Resolver {
 fn resolve(&mut self,name:&str,max:u64)->ms_package::Result<Vec<u8>> {let bytes=std::fs::read(self.0.join(name))?;if bytes.len() as u64>max{return Err(ms_package::Error::Limit("media bytes"))}Ok(bytes)}
}
fn main()->Result<(),Box<dyn std::error::Error>>{
 let root=PathBuf::from(std::env::args().nth(1).unwrap());
 let mut cell=0;
 for arch in [InstallerArchitecture::X86,InstallerArchitecture::X64] {for context in [InstallationContext::PerUser,InstallationContext::PerMachine] {for mode in ["embedded","external","loose","split"] {
 cell+=1;
 let name=format!("{}-{}-{mode}",if arch==InstallerArchitecture::X86{"x86"}else{"x64"},if context==InstallationContext::PerUser{"user"}else{"machine"});
 let dir=root.join(&name);std::fs::create_dir_all(&dir)?;
 let identity=InstallerIdentity{product_code:guid(cell,1),package_code:guid(cell,2),upgrade_code:guid(cell,3),name:format!("MSI matrix {name}"),manufacturer:"ms-package".into(),version:"1.0.0".into(),directory_name:format!("ms-package-matrix-{name}"),architecture:arch,context};
 let mut b=InstallerBuilder::new(identity,WriteOptions::default())?;
 let payload=format!("qualification payload {name}\r\n");std::fs::write(dir.join("payload-reference.txt"),&payload)?;
 b.add_file("Payload","payload.txt",&guid(cell,4),Cursor::new(payload))?;
 let layout=match mode{"embedded"=>InstallerMediaLayout::Embedded,"external"=>InstallerMediaLayout::ExternalCabinet{name:"payload.cab".into()},"loose"=>InstallerMediaLayout::Loose,_=>{
 let second=format!("second qualification payload {name}\r\n");std::fs::write(dir.join("second-reference.txt"),&second)?;
 b.add_file("Second","second.txt",&guid(cell,5),Cursor::new(second))?;
 InstallerMediaLayout::Cabinets{cabinets:vec![InstallerCabinetSpec{name:"first.cab".into(),file_count:1,embedded:false},InstallerCabinetSpec{name:"second.cab".into(),file_count:1,embedded:false}]}}};
 b.write_with_media(layout,&mut Sink(dir.clone()),std::fs::File::create(dir.join("package.msi"))?)?;
 println!("{name}");
 }}}
 for (cell,mode) in [(17,"edited-external"),(18,"edited-mixed")] {
 let source=root.join(format!("source-{mode}"));std::fs::create_dir_all(&source)?;
 let output=root.join(format!("x64-user-{mode}"));std::fs::create_dir_all(&output)?;
 let id=InstallerIdentity{product_code:guid(cell,1),package_code:guid(cell,2),upgrade_code:guid(cell,3),name:format!("MSI matrix {mode}"),manufacturer:"ms-package".into(),version:"1.0.0".into(),directory_name:format!("ms-package-matrix-x64-user-{mode}"),architecture:InstallerArchitecture::X64,context:InstallationContext::PerUser};
 let mut b=InstallerBuilder::new(id,WriteOptions::default())?;
 b.add_file("Payload","payload.txt",&guid(cell,4),Cursor::new(b"original data"))?;
 let layout=if cell==17{InstallerMediaLayout::ExternalCabinet{name:"payload.cab".into()}}else{
 b.add_file("Second","second.txt",&guid(cell,5),Cursor::new(b"second mixed payload"))?;
 std::fs::write(output.join("second-reference.txt"),b"second mixed payload")?;
 InstallerMediaLayout::Cabinets{cabinets:vec![InstallerCabinetSpec{name:"first.cab".into(),file_count:1,embedded:true},InstallerCabinetSpec{name:"second.cab".into(),file_count:1,embedded:false}]}};
 let mut original=Vec::new();b.write_with_media(layout.clone(),&mut Sink(source.clone()),&mut original)?;
 std::fs::write(source.join("package.msi"),&original)?;
 let mut editor=InstallerPayloadEditor::open_with_media(Cursor::new(original),&guid(cell,6),WriteOptions::default(),&mut Resolver(source))?;
 let replacement=format!("edited qualification payload {mode}\r\n");std::fs::write(output.join("payload-reference.txt"),&replacement)?;
 editor.replace_file("Payload",Cursor::new(replacement))?;
 editor.write_with_media(layout,&mut Sink(output.clone()),std::fs::File::create(output.join("package.msi"))?)?;
 }
 Ok(())
}
