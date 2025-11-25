use bevy::{
    asset::Asset,
    image::{TextureAtlasBuilder, TextureAtlasBuilderError},
    math::UVec2,
    prelude::*,
    reflect::TypePath,
};
use std::collections::HashMap;

use brine_asset::TextureKey;

#[derive(Debug, Clone, Asset, TypePath)]
pub struct TextureAtlas {
    /// The handle to the stitched texture atlas.
    pub texture: Handle<Image>,

    /// Mapping from texture key to UV coordinate within the atlas (`0.0` to
    /// `1.0` scale).
    pub regions: HashMap<TextureKey, Rect>,

    /// The texture atlas will always contain a placeholder texture in one of
    /// the regions. This stores that region.
    pub placeholder_region: Rect,
}

impl TextureAtlas {
    /// Returns the UV coordinates within the stitched atlas at which the given
    /// texture can be found.
    ///
    /// If the given texture is not in the atlas, the UV coordinates will
    /// correspond to some placeholder texture in the atlas.
    pub fn get_uv(&self, texture: TextureKey) -> Rect {
        self.regions
            .get(&texture)
            .copied()
            .unwrap_or(self.placeholder_region)
    }

    pub fn stitch<'a, T>(
        assets: &mut Assets<Image>,
        textures: T,
        placeholder_texture: &Handle<Image>,
        max_texture_size: u32,
    ) -> Result<Self, TextureAtlasBuilderError>
    where
        T: IntoIterator<Item = (TextureKey, &'a Handle<Image>)>,
    {
        let textures: Vec<(TextureKey, &Handle<Image>)> = textures.into_iter().collect();

        debug!("Stitching texture atlas with {} textures", textures.len());

        let mut builder = TextureAtlasBuilder::default();
        builder.max_size(UVec2::new(max_texture_size, max_texture_size));

        for (_, handle) in textures.iter() {
            let image = assets.get(*handle).expect("all textures must be loaded");
            builder.add_texture(Some(handle.id()), image);
        }

        builder.add_texture(
            Some(placeholder_texture.id()),
            assets.get(placeholder_texture).unwrap(),
        );

        let (layout, sources, atlas_image) = builder.build()?;
        let atlas_size = layout.size.as_vec2();
        let atlas_handle = assets.add(atlas_image);

        let handle_to_uv = |handle: &Handle<Image>| {
            sources
                .uv_rect(&layout, handle.id())
                .expect("texture missing from atlas")
        };

        let key_to_uv = textures
            .iter()
            .map(|(key, handle)| (*key, handle_to_uv(handle)))
            .collect();

        let placeholder_uv = handle_to_uv(placeholder_texture);

        debug!(
            "Done. Final atlas size: {} x {}",
            atlas_size.x as u32, atlas_size.y as u32
        );

        Ok(Self {
            texture: atlas_handle,
            regions: key_to_uv,
            placeholder_region: placeholder_uv,
        })
    }

    /// Build an atlas that maps the provided texture keys to the placeholder
    /// texture. This is a defensive fallback when stitching fails and ensures
    /// that every requested texture key is still routable.
    pub fn placeholder_only<I>(placeholder_texture: &Handle<Image>, texture_keys: I) -> Self
    where
        I: IntoIterator<Item = TextureKey>,
    {
        let mut regions = HashMap::new();
        let placeholder_region = Rect::from_corners(Vec2::ZERO, Vec2::ONE);

        for key in texture_keys.into_iter() {
            regions.insert(key, placeholder_region);
        }

        Self {
            texture: placeholder_texture.clone(),
            regions,
            placeholder_region,
        }
    }
}

#[derive(Debug)]
pub(crate) struct PendingAtlas {
    /// Strong handle to each texture that will eventually be added to the atlas.
    pub textures: Vec<(TextureKey, Handle<Image>)>,

    /// Strong handle that we will eventually populate with a built atlas.
    // TODO: should be weak?
    pub handle: Handle<TextureAtlas>,
}

impl PendingAtlas {
    pub fn all_textures_loaded(&self, assets: &Assets<Image>) -> bool {
        self.textures
            .iter()
            .all(|(_, handle)| assets.contains(handle))
    }
}
