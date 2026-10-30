//! Read-only content raster adapter. No file/decoder/COM ownership enters input
//! events. Every retained pixel owner remains charged by RasterCache's permit.
use super::*;
use fc_presentation::{card_body::ImageBodyGeometry, scene::SceneNodeKey};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageRenderRequest {
    pub identity: SceneContentIdentity,
    pub reference: MediaReference,
    pub max_pixel_width: u32,
    pub max_pixel_height: u32,
}
fn invalid() -> Error {
    Error::from_hresult(E_INVALIDARG)
}
fn geometry(card: &SceneCard) -> Result<ImageBodyGeometry> {
    card_body_spec(card.kind, &card.content, false, None)
        .map_err(|_| invalid())?
        .image_geometry(card.rect.width, card.rect.height)
        .map_err(|_| invalid())
}
pub(super) fn validate_identity(card: &SceneCard) -> Result<()> {
    match (&card.content, card.content_identity, card.kind) {
        (CardContent::StandaloneText(text), Some(id), ComponentKind::SubjectText) => {
            if id.revision.get() == 0
                || id.view_revision < id.revision
                || text.chars().count() > 10_000
                || text.len() > 40_000
                || text
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            {
                return Err(invalid());
            }
        }
        (CardContent::Image(image), Some(id), ComponentKind::SubjectImage) => {
            image.current.validate().map_err(|_| invalid())?;
            if id.revision.get() == 0
                || id.view_revision < id.revision
                || image.current.owner != id.slot
                || image.total_pages == 0
                || image.total_pages > 50
                || image.current_page >= image.total_pages
            {
                return Err(invalid());
            }
            geometry(card)?;
        }
        (CardContent::StandaloneText(_) | CardContent::Image(_), _, _)
        | (_, _, ComponentKind::SubjectText | ComponentKind::SubjectImage) => return Err(invalid()),
        (_, None, _) => (),
        (_, Some(_), _) => return Err(invalid()),
    }
    Ok(())
}
pub(super) fn requests(scene: &SceneSnapshot) -> Result<Vec<ImageRenderRequest>> {
    if scene.cards.len() > 64 {
        return Err(invalid());
    }
    let mut out = Vec::new();
    for (index, card) in scene.cards.iter().enumerate() {
        validate_identity(card)?;
        if let Some(identity) = card.content_identity {
            let tree = scene.paint_tree.as_ref().ok_or_else(invalid)?;
            if !tree.nodes.iter().any(|node| {
                node.card_index == Some(index)
                    && node.key
                        == SceneNodeKey {
                            scope: identity.slot.scope,
                            id: identity.slot.node_id,
                        }
            }) {
                return Err(invalid());
            }
        }
        let CardContent::Image(image) = &card.content else {
            continue;
        };
        let identity = card.content_identity.ok_or_else(invalid)?;
        // Independent content is always rooted in an actual scoped tree node.
        let tree = scene.paint_tree.as_ref().ok_or_else(invalid)?;
        if !tree.nodes.iter().any(|node| {
            node.card_index == Some(index)
                && node.key
                    == SceneNodeKey {
                        scope: identity.slot.scope,
                        id: identity.slot.node_id,
                    }
        }) {
            return Err(invalid());
        }
        let image_rect = geometry(card)?.image;
        out.push(ImageRenderRequest {
            identity,
            reference: image.current.clone(),
            max_pixel_width: image_rect.width.ceil().max(1.) as u32,
            max_pixel_height: image_rect.height.ceil().max(1.) as u32,
        });
    }
    Ok(out)
}
pub(super) fn frame(cache: &RasterCache, card: &SceneCard) -> Result<Option<Arc<RasterFrame>>> {
    let CardContent::Image(image) = &card.content else {
        return Ok(None);
    };
    let area = geometry(card)?.image;
    cache
        .get(
            &image.current,
            area.width.ceil().max(1.) as u32,
            area.height.ceil().max(1.) as u32,
        )?
        .map(Some)
        .ok_or_else(|| Error::from_hresult(ERROR_NOT_READY.to_hresult()))
}
pub(super) fn valid_page(card: &SceneCard, identity: SceneContentIdentity, page: u8) -> bool {
    card.content_identity == Some(identity)
        && matches!(&card.content, CardContent::Image(image)
        if page < image.total_pages && page != image.current_page && image.current.owner == identity.slot)
}
pub(super) fn image_targets(card: &SceneCard) -> Result<Vec<HitTarget>> {
    let CardContent::Image(image) = &card.content else {
        return Ok(Vec::new());
    };
    let id = card.content_identity.ok_or_else(invalid)?;
    let Some(pager) = geometry(card)?.pager else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    if image.current_page > 0 {
        out.push(HitTarget {
            rect: pager.previous,
            action: InputAction::ImagePage {
                identity: id,
                page: image.current_page - 1,
            },
        });
    }
    if image.current_page + 1 < image.total_pages {
        out.push(HitTarget {
            rect: pager.next,
            action: InputAction::ImagePage {
                identity: id,
                page: image.current_page + 1,
            },
        });
    }
    Ok(out)
}
pub(super) fn draw_pager(
    g: &Graphics,
    target: &ID2D1DCRenderTarget,
    card: &SceneCard,
    tone: TextTone,
) -> Result<()> {
    let CardContent::Image(image) = &card.content else {
        return Ok(());
    };
    let Some(pager) = geometry(card)?.pager else {
        return Ok(());
    };
    let layout = g.layout(
        &format!("‹   {}/{}   ›", image.current_page + 1, image.total_pages),
        pager.rect.width,
        13.,
        true,
    )?;
    // SAFETY: local target and layout are borrowed only during this UI-thread
    // draw; all coordinates come from the same validated CardBodySpec geometry.
    unsafe {
        let brush = target.CreateSolidColorBrush(&ink(tone, card.component_alpha), None)?;
        target.DrawTextLayout(
            xy(pager.rect.x, pager.rect.y + 14.).point,
            &layout,
            &brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
    }
    Ok(())
}
pub(super) fn blend_image(card: &mut Card) -> Result<()> {
    if !matches!(card.source.content, CardContent::Image(_)) {
        return Ok(());
    }
    let image = geometry(&card.source)?.image;
    let frame = card.image.as_ref().ok_or_else(invalid)?;
    let key = frame.key();
    let stride = card.surface.dib.width as usize * 4;
    let width = card.surface.dib.width as usize;
    let height = card.surface.dib.height as usize;
    let (x0, y0, x1, y1) = pixel_bounds(image, width, height)?;
    if x1 > width || y1 > height || x1 <= x0 || y1 <= y0 {
        return Err(invalid());
    }
    blend_pixels(
        card.surface.dib.pixels_mut(),
        stride,
        (x0, y0, x1, y1),
        frame.pixels(),
        frame.stride(),
        key.width as usize,
        key.height as usize,
        card.source.component_alpha,
    );
    Ok(())
}
// A legal extreme aspect ratio may cover less than one physical pixel.
// Conservative coverage retains that line instead of rejecting valid media.
fn pixel_bounds(image: Rect, width: usize, height: usize) -> Result<(usize, usize, usize, usize)> {
    if width == 0
        || height == 0
        || image.width <= 0.
        || image.height <= 0.
        || ![image.x, image.y, image.width, image.height]
            .iter()
            .all(|n| n.is_finite())
    {
        return Err(invalid());
    }
    let x0 = image.x.floor().max(0.) as usize;
    let y0 = image.y.floor().max(0.) as usize;
    let x1 = (image.x + image.width).ceil().max(0.) as usize;
    let y1 = (image.y + image.height).ceil().max(0.) as usize;
    if x0 >= width || y0 >= height {
        return Err(invalid());
    }
    Ok((
        x0,
        y0,
        x1.max(x0 + 1).min(width),
        y1.max(y0 + 1).min(height),
    ))
}
/// Source-over on premultiplied BGRA. The only destination is the already
/// reserved card DIB; scaling reads the immutable cache with no extra bitmap.
#[allow(clippy::too_many_arguments)]
fn blend_pixels(
    dst: &mut [u8],
    stride: usize,
    rect: (usize, usize, usize, usize),
    src: &[u8],
    src_stride: usize,
    sw: usize,
    sh: usize,
    opacity: f32,
) {
    let (x0, y0, x1, y1) = rect;
    let alpha = (opacity * 255.).round() as u32;
    for y in y0..y1 {
        for x in x0..x1 {
            let sx = (x - x0) * sw / (x1 - x0);
            let sy = (y - y0) * sh / (y1 - y0);
            let source = &src[sy * src_stride + sx * 4..][..4];
            let dest = &mut dst[y * stride + x * 4..][..4];
            let a = (u32::from(source[3]) * alpha + 127) / 255;
            for channel in 0..4 {
                let s = (u32::from(source[channel]) * alpha + 127) / 255;
                dest[channel] =
                    (s + (u32::from(dest[channel]) * (255 - a) + 127) / 255).min(255) as u8;
            }
        }
    }
}

#[cfg(test)]
#[path = "content_tests.rs"]
mod tests;
