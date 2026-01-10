use eframe::egui;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::collections::HashSet;
use std::thread;
use crate::ggpk::reader::GgpkReader;
use crate::bundles::index::{Index, FileInfo};

pub struct TextureLoader {
    request_tx: Sender<(u64, String, Arc<GgpkReader>, Arc<Index>, FileInfo)>,
    result_rx: Receiver<(u64, egui::ColorImage)>,
    pending: HashSet<u64>,
}

impl TextureLoader {
    pub fn new() -> Self {
        let (request_tx, request_rx) = channel::<(u64, String, Arc<GgpkReader>, Arc<Index>, FileInfo)>();
        let (result_tx, result_rx) = channel();

        thread::spawn(move || {
            while let Ok((hash, _path, reader, index, file_info)) = request_rx.recv() {
                // Determine bundle options
                if let Some(bundle_info) = index.bundles.get(file_info.bundle_index as usize) {
                    let candidates = vec![
                        format!("Bundles2/{}", bundle_info.name),
                        format!("Bundles2/{}.bundle.bin", bundle_info.name),
                        bundle_info.name.clone(),
                        format!("{}.bundle.bin", bundle_info.name),
                    ];
                    
                    let mut raw_data = None;
                    
                    // 1. Try Local GGPK
                    for cand in &candidates {
                        if let Ok(Some(rec)) = reader.read_file_by_path(cand) {
                            if let Ok(data) = reader.get_data_slice(rec.data_offset, rec.data_length) {
                                raw_data = Some(data.to_vec());
                                break;
                            }
                        }
                    }
                    
                    // Note: We skip CDN fallback here to simplify threading deps (CdnBundleLoader is not strictly Send/Sync compatible easily without Arc).
                    // For thumbnails, we assume most are valid in GGPK. If needed we can pass CdnBundleLoader later.

                    if let Some(data) = raw_data {
                         let mut cursor = std::io::Cursor::new(data);
                         if let Ok(bundle) = crate::bundles::bundle::Bundle::read_header(&mut cursor) {
                             if let Ok(decompressed) = bundle.decompress(&mut cursor) {
                                 let start = file_info.file_offset as usize;
                                 let end = start + file_info.file_size as usize;
                                 if end <= decompressed.len() {
                                     let file_data = &decompressed[start..end];
                                     
                                     // Attempt DDS conversion
                                     // Since image_dds and image crates are used in main thread, we duplicate logic here slightly
                                     // Method 1: ddsfile + image_dds
                                     let mut image_res = None;
                                     let mut cursor = std::io::Cursor::new(file_data);
                                     if let Ok(dds) = ddsfile::Dds::read(&mut cursor) {
                                         if let Ok(image) = image_dds::image_from_dds(&dds, 0) {
                                              image_res = Some(image);
                                         }
                                     }
                                     
                                     // Method 2: image crate
                                     if image_res.is_none() {
                                         if let Ok(img) = image::load_from_memory(file_data) {
                                             image_res = Some(img.to_rgba8());
                                         }
                                     }

                                     if let Some(img) = image_res {
                                         let size = [img.width() as usize, img.height() as usize];
                                         let pixels = img.as_flat_samples();
                                         let color_image = egui::ColorImage::from_rgba_unmultiplied(
                                             size,
                                             pixels.as_slice(),
                                         );
                                         let _ = result_tx.send((hash, color_image));
                                     }
                                 }
                             }
                         }
                    }
                }
            }
        });

        Self {
            request_tx,
            result_rx,
            pending: HashSet::new(),
        }
    }

    pub fn request(&mut self, hash: u64, path: String, reader: Arc<GgpkReader>, index: Arc<Index>, file_info: &FileInfo) {
        if self.pending.contains(&hash) {
            return;
        }
        self.pending.insert(hash);
        let _ = self.request_tx.send((hash, path, reader, index, file_info.clone()));
    }

    pub fn poll(&mut self) -> Option<(u64, egui::ColorImage)> {
        if let Ok((hash, image)) = self.result_rx.try_recv() {
            self.pending.remove(&hash);
            Some((hash, image))
        } else {
            None
        }
    }
    
    pub fn is_loading(&self, hash: u64) -> bool {
        self.pending.contains(&hash)
    }
}
