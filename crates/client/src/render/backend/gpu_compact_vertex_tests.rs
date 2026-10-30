use super::{GpuBackend, GpuContext, GPU_SCENE_LOCK, SCENE_H, SCENE_W};
use crate::config::Cache;
use crate::dash3d::Model;
use crate::graphics::{Pix2D, Pix3D, Pix3DDraw};
use crate::io::JagFile;
use crate::render::backend::TextureHandle;
use crate::render::budget::{RenderBudget, RenderBudgetLimits, RenderLimit, RenderResource};
use crate::render::world::{GpuVertex, SceneMesh};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::{Path, PathBuf};

const SNAPSHOT_ENV: &str = "FR_CLIENT_SNAPSHOT";
const MODE_ENV: &str = "T44_COMPACT_VERTEX_MODE";
const MODE_OFF: &str = "off";
const MODE_ON: &str = "on";
const MIB: usize = 1024 * 1024;
const RETAINED_VERTEX_CAP: usize = 128 * MIB;
const PEAK_VERTEX_LIMIT: usize = 192 * MIB;
const MAX_FIXTURE_VERTICES: usize = 500_000;
const MIN_NON_BACKGROUND_PIXELS: usize = 500;
const TARGET_FACE_TRIES_PER_YAW: usize = 4;
const MODEL_YAWS: [i32; 8] = [0, 256, 512, 768, 1024, 1280, 1536, 1792];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AlphaKind {
    Untextured,
    Textured,
}

impl fmt::Display for AlphaKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Untextured => "untextured",
            Self::Textured => "textured",
        })
    }
}

#[derive(Debug)]
enum ProofFailure {
    MissingEnvironment(&'static str),
    InvalidMode(String),
    SnapshotPath {
        path: PathBuf,
        reason: String,
    },
    ReadFile {
        path: PathBuf,
        reason: String,
    },
    InvalidModelArchive(String),
    MissingModel(i32),
    MissingLocationModel,
    MissingAlphaModel(AlphaKind),
    GpuUnavailable(String),
    Capture(RenderLimit),
    Render(RenderLimit),
    VertexPreflight(String),
    PixelMismatch {
        label: String,
        differing_pixels: usize,
        first: Option<(usize, i32, i32)>,
    },
    InsufficientCoverage {
        label: String,
        pixels: usize,
    },
    FixtureTooLarge {
        label: String,
        vertices: usize,
    },
    BudgetMode(String),
}

impl fmt::Display for ProofFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnvironment(name) => write!(f, "required environment variable {name} is missing"),
            Self::InvalidMode(value) => write!(f, "{MODE_ENV} must be exactly '{MODE_OFF}' or '{MODE_ON}', got {value:?}"),
            Self::SnapshotPath { path, reason } => write!(f, "invalid {SNAPSHOT_ENV} snapshot {}: {reason}", path.display()),
            Self::ReadFile { path, reason } => write!(f, "cannot read real-cache proof input {}: {reason}", path.display()),
            Self::InvalidModelArchive(reason) => write!(f, "invalid models.bin fixture: {reason}"),
            Self::MissingModel(id) => write!(f, "models.bin has no decodable model {id}"),
            Self::MissingLocationModel => f.write_str("config cache does not contain location 879's model"),
            Self::MissingAlphaModel(kind) => write!(f, "models.bin contains no renderable near-clipped {kind} face with source alpha in 2..=254"),
            Self::GpuUnavailable(reason) => write!(f, "the real-cache pixel proof requires a usable GPU: {reason}"),
            Self::Capture(error) => write!(f, "CPU model capture failed: {error}"),
            Self::Render(error) => write!(f, "compact production scene render failed: {error}"),
            Self::VertexPreflight(reason) => write!(f, "vertex-storage preflight contract failed: {reason}"),
            Self::PixelMismatch { label, differing_pixels, first } => write!(f, "{label}: compact/legacy24 images differ at {differing_pixels} pixels; first (index, compact, legacy)={first:?}"),
            Self::InsufficientCoverage { label, pixels } => write!(f, "{label}: only {pixels} non-background pixels; expected at least {MIN_NON_BACKGROUND_PIXELS}"),
            Self::FixtureTooLarge { label, vertices } => write!(f, "{label}: {vertices} captured vertices exceeds the bounded real-cache proof limit {MAX_FIXTURE_VERTICES}"),
            Self::BudgetMode(reason) => write!(f, "render-budget scope contract failed: {reason}"),
        }
    }
}

impl std::error::Error for ProofFailure {}

#[derive(Clone, Copy, Debug)]
struct Camera {
    model_yaw: i32,
    sin_eye_pitch: i32,
    cos_eye_pitch: i32,
    sin_eye_yaw: i32,
    cos_eye_yaw: i32,
    relative_x: i32,
    relative_y: i32,
    relative_z: i32,
}

impl Camera {
    fn level(model_yaw: i32, relative_x: i32, relative_y: i32, relative_z: i32) -> Self {
        Self {
            model_yaw,
            sin_eye_pitch: 0,
            cos_eye_pitch: 65536,
            sin_eye_yaw: 0,
            cos_eye_yaw: 65536,
            relative_x,
            relative_y,
            relative_z,
        }
    }
}

struct CaseDescriptor {
    label: String,
    model_id: i32,
    model_record_sha256: String,
    camera: Camera,
    target_face: Option<usize>,
    target_face_view_z: Option<[i32; 3]>,
}

struct ProofCase {
    label: String,
    model_id: i32,
    model_record_sha256: String,
    camera: Camera,
    source_points: usize,
    source_faces: usize,
    source_alpha_faces: usize,
    source_alpha_untextured_faces: usize,
    source_alpha_textured_faces: usize,
    source_near_faces: usize,
    target_face: Option<usize>,
    target_face_view_z: Option<[i32; 3]>,
    mesh: SceneMesh,
}

#[derive(Default)]
struct MeshMetrics {
    triangles: usize,
    alpha_triangles: usize,
    alpha_untextured_triangles: usize,
    alpha_textured_triangles: usize,
    near_boundary_vertices: usize,
    near_boundary_triangles: usize,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Legacy24Vertex {
    x: f32,
    y: f32,
    z: f32,
    abhsl: u32,
    uv_tex: u32,
    v: u32,
}

const _: [(); 24] = [(); std::mem::size_of::<Legacy24Vertex>()];
const _: [(); 16] = [(); std::mem::size_of::<GpuVertex>()];

struct Legacy24Pipeline {
    pipeline: wgpu::RenderPipeline,
    scene_layout: wgpu::BindGroupLayout,
}

#[test]
#[ignore = "requires FR_CLIENT_SNAPSHOT and a usable GPU; run once with each T44_COMPACT_VERTEX_MODE"]
fn real_cache_compact_vertex_pixels_match_pinned_legacy24() {
    let mode = std::env::var(MODE_ENV)
        .unwrap_or_else(|_| panic!("{MODE_ENV} is required; run isolated 'off' and 'on' cells"));
    let result = match mode.as_str() {
        MODE_OFF => {
            if RenderBudget::current().is_some() {
                Err(ProofFailure::BudgetMode(
                    "off mode must begin with no active RenderBudget".into(),
                ))
            } else {
                run_proof(MODE_OFF)
            }
        }
        MODE_ON => {
            if RenderBudget::current().is_some() {
                Err(ProofFailure::BudgetMode(
                    "on mode must install its explicit fixture before GPU context creation".into(),
                ))
            } else {
                let budget = fixture_budget(PEAK_VERTEX_LIMIT);
                let scope = budget.scope();
                let run = run_proof(MODE_ON);
                drop(scope);
                if budget.for_resource(RenderResource::Vertex).live() != 0 {
                    Err(ProofFailure::BudgetMode(format!(
                        "backend drop left {} bytes charged to the outer Vertex fixture",
                        budget.for_resource(RenderResource::Vertex).live()
                    )))
                } else {
                    run
                }
            }
        }
        other => Err(ProofFailure::InvalidMode(other.to_owned())),
    };
    if let Err(error) = result {
        panic!("{error}");
    }
}

fn fixture_budget(vertex_peak: usize) -> RenderBudget {
    let finite = 256 * MIB;
    RenderBudget::fixture(RenderBudgetLimits::new(
        finite,
        finite,
        finite,
        finite,
        finite,
        finite,
        finite,
        finite,
        finite,
        finite,
        vertex_peak,
        finite,
        finite,
    ))
}

fn run_proof(mode: &str) -> Result<(), ProofFailure> {
    let snapshot = std::env::var_os(SNAPSHOT_ENV)
        .map(PathBuf::from)
        .ok_or(ProofFailure::MissingEnvironment(SNAPSHOT_ENV))?;
    if !snapshot.is_dir() {
        return Err(ProofFailure::SnapshotPath {
            path: snapshot,
            reason: "expected a directory containing models.bin, config, and textures".into(),
        });
    }

    let models = read_snapshot_file(&snapshot, "models.bin")?;
    let config_bytes = read_snapshot_file(&snapshot, "config")?;
    let texture_bytes = read_snapshot_file(&snapshot, "textures")?;
    let model_archive_hash = sha256_hex(&models);
    let config_hash = sha256_hex(&config_bytes);
    let texture_archive_hash = sha256_hex(&texture_bytes);
    let model_1497 = find_model_record(&models, 1497)?.ok_or(ProofFailure::MissingModel(1497))?;

    Pix3D::init_colour_table(0.8);
    Model::reset_for_tests();
    Model::unpack(1497, Some(model_1497));
    let cache = Cache::unpack(&JagFile::new(config_bytes));
    if cache.locs.len() <= 879 {
        return Err(ProofFailure::MissingLocationModel);
    }
    let fountain_model = cache.locs[879]
        .get_model(&cache, 10, 0, 0, 0, 0, 0, -1)
        .ok_or(ProofFailure::MissingLocationModel)?;
    if fountain_model.num_faces != 302 {
        return Err(ProofFailure::BudgetMode(format!(
            "pinned loc 879 model 1497 changed face count from 302 to {}",
            fountain_model.num_faces
        )));
    }

    let texture_pack = JagFile::new(texture_bytes);
    let mut pix = Pix3DDraw::default();
    pix.unpack_textures(&texture_pack);
    pix.init_pool(20);
    pix.init_texture_palettes(0.8);

    let mut cases = Vec::with_capacity(3);
    let fountain_camera = fountain_camera(256);
    cases.push(capture_case(
        &fountain_model,
        &mut pix,
        CaseDescriptor {
            label: "loc879-model1497-pitch256".into(),
            model_id: 1497,
            model_record_sha256: sha256_hex(model_1497),
            camera: fountain_camera,
            target_face: None,
            target_face_view_z: None,
        },
    )?);
    cases.push(find_translucent_case(
        &models,
        &mut pix,
        AlphaKind::Untextured,
    )?);
    cases.push(find_translucent_case(
        &models,
        &mut pix,
        AlphaKind::Textured,
    )?);

    if std::mem::size_of::<GpuVertex>() != 16 || std::mem::size_of::<Legacy24Vertex>() != 24 {
        return Err(ProofFailure::BudgetMode(
            "pixel oracle is not using the 16-byte/24-byte record pair".into(),
        ));
    }

    let mut backend = GpuBackend::try_new().map_err(ProofFailure::GpuUnavailable)?;
    let oracle = Legacy24Pipeline::new(&backend.context);
    for case in &cases {
        compare_case(&mut backend, &oracle, &mut pix, case, mode)?;
    }
    check_vertex_preflight(&mut backend)?;

    println!(
        "T44_COMPACT_PIXEL_PROOF mode={mode} budget_receipt=backend-budget-scope-{mode} cache_sha256={{models.bin:{model_archive_hash},config:{config_hash},textures:{texture_archive_hash}}} cases={} model_archive_records={} retained_vertex_cap={}MiB peak_vertex_fixture={}MiB",
        cases.len(),
        model_record_count(&models)?,
        RETAINED_VERTEX_CAP / MIB,
        PEAK_VERTEX_LIMIT / MIB,
    );
    Ok(())
}

fn read_snapshot_file(snapshot: &Path, name: &str) -> Result<Vec<u8>, ProofFailure> {
    let path = snapshot.join(name);
    std::fs::read(&path).map_err(|error| ProofFailure::ReadFile {
        path,
        reason: error.to_string(),
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use fmt::Write as _;
        write!(&mut hex, "{byte:02x}").expect("writing to String cannot fail");
    }
    hex
}

fn find_model_record(bytes: &[u8], wanted: i32) -> Result<Option<&[u8]>, ProofFailure> {
    let mut offset = 0usize;
    while offset < bytes.len() {
        let (id, record, next) = parse_model_record(bytes, offset)?;
        if id == wanted {
            return Ok(Some(record));
        }
        offset = next;
    }
    Ok(None)
}

fn model_record_count(bytes: &[u8]) -> Result<usize, ProofFailure> {
    let mut offset = 0usize;
    let mut count = 0usize;
    while offset < bytes.len() {
        let (_, _, next) = parse_model_record(bytes, offset)?;
        offset = next;
        count += 1;
    }
    Ok(count)
}

fn parse_model_record(bytes: &[u8], offset: usize) -> Result<(i32, &[u8], usize), ProofFailure> {
    let header_end = offset
        .checked_add(8)
        .ok_or_else(|| ProofFailure::InvalidModelArchive("record header offset overflow".into()))?;
    let header = bytes.get(offset..header_end).ok_or_else(|| {
        ProofFailure::InvalidModelArchive(format!("truncated record header at byte {offset}"))
    })?;
    let id = i32::try_from(u32::from_le_bytes(header[0..4].try_into().unwrap())).map_err(|_| {
        ProofFailure::InvalidModelArchive(format!("model id exceeds i32 at byte {offset}"))
    })?;
    let length =
        usize::try_from(u32::from_le_bytes(header[4..8].try_into().unwrap())).map_err(|_| {
            ProofFailure::InvalidModelArchive(format!(
                "record length exceeds usize at byte {offset}"
            ))
        })?;
    let end = header_end.checked_add(length).ok_or_else(|| {
        ProofFailure::InvalidModelArchive(format!("model {id} length overflows at byte {offset}"))
    })?;
    let record = bytes.get(header_end..end).ok_or_else(|| {
        ProofFailure::InvalidModelArchive(format!("model {id} is truncated at byte {offset}"))
    })?;
    Ok((id, record, end))
}

fn fountain_camera(pitch: i32) -> Camera {
    let distance = pitch * 3 + 600;
    let inverse = ((2048 - pitch) & 2047) as usize;
    let eye_y = -295 - ((-distance * Pix3D::sin_table()[inverse]) >> 16);
    let eye_z = 7232 - ((distance * Pix3D::cos_table()[inverse]) >> 16);
    Camera {
        model_yaw: 0,
        sin_eye_pitch: Pix3D::sin_table()[pitch as usize],
        cos_eye_pitch: Pix3D::cos_table()[pitch as usize],
        sin_eye_yaw: 0,
        cos_eye_yaw: 65536,
        relative_x: 192,
        relative_y: -eye_y,
        relative_z: 7552 - eye_z,
    }
}

fn capture_case(
    model: &Model,
    pix: &mut Pix3DDraw,
    descriptor: CaseDescriptor,
) -> Result<ProofCase, ProofFailure> {
    let mesh = capture_model(model, pix, descriptor.camera)?;
    case_from_mesh(model, descriptor, mesh)
}

fn case_from_mesh(
    model: &Model,
    descriptor: CaseDescriptor,
    mesh: SceneMesh,
) -> Result<ProofCase, ProofFailure> {
    let source_points = usize::try_from(model.num_points).unwrap_or(0);
    let source_faces = usize::try_from(model.num_faces).unwrap_or(0);
    let (source_alpha_faces, source_alpha_untextured_faces, source_alpha_textured_faces) =
        source_alpha_counts(model);
    let source_near_faces = source_near_face_count(model, descriptor.camera);
    if mesh.vertices().len() > MAX_FIXTURE_VERTICES {
        return Err(ProofFailure::FixtureTooLarge {
            label: descriptor.label,
            vertices: mesh.vertices().len(),
        });
    }
    if mesh.vertices().len() % 3 != 0 {
        return Err(ProofFailure::BudgetMode(format!(
            "{} capture is not a triangle list ({} vertices)",
            descriptor.label,
            mesh.vertices().len()
        )));
    }
    if mesh.retained_bytes() > RETAINED_VERTEX_CAP {
        return Err(ProofFailure::BudgetMode(format!(
            "{} capture retains {} bytes, above the 128 MiB capture cap",
            descriptor.label,
            mesh.retained_bytes()
        )));
    }
    Ok(ProofCase {
        label: descriptor.label,
        model_id: descriptor.model_id,
        model_record_sha256: descriptor.model_record_sha256,
        camera: descriptor.camera,
        source_points,
        source_faces,
        source_alpha_faces,
        source_alpha_untextured_faces,
        source_alpha_textured_faces,
        source_near_faces,
        target_face: descriptor.target_face,
        target_face_view_z: descriptor.target_face_view_z,
        mesh,
    })
}

fn capture_model(
    model: &Model,
    pix: &mut Pix3DDraw,
    camera: Camera,
) -> Result<SceneMesh, ProofFailure> {
    let mut pixels = vec![0i32; (SCENE_W * SCENE_H) as usize];
    let mut surface = Pix2D::with_pixels(&mut pixels, SCENE_W as i32, SCENE_H as i32);
    pix.set_render_clipping(&surface);
    let _ = pix.capture.take();
    pix.capture = Some(SceneMesh::default());
    model
        .world_render(
            pix,
            &mut surface,
            camera.model_yaw,
            camera.sin_eye_pitch,
            camera.cos_eye_pitch,
            camera.sin_eye_yaw,
            camera.cos_eye_yaw,
            camera.relative_x,
            camera.relative_y,
            camera.relative_z,
            0,
        )
        .map_err(ProofFailure::Capture)?;
    pix.capture.take().ok_or_else(|| {
        ProofFailure::BudgetMode("world_render did not return its requested capture".into())
    })
}

fn source_alpha_counts(model: &Model) -> (usize, usize, usize) {
    let mut total = 0;
    let mut untextured = 0;
    let mut textured = 0;
    let face_alpha = model.face_alpha.as_deref();
    let render_type = model.face_render_type.as_deref();
    for face in 0..usize::try_from(model.num_faces).unwrap_or(0) {
        let alpha = face_alpha.and_then(|a| a.get(face)).copied().unwrap_or(0);
        if !(2..=254).contains(&alpha)
            || render_type.and_then(|t| t.get(face)).copied().unwrap_or(0) == -1
        {
            continue;
        }
        total += 1;
        if (render_type.and_then(|t| t.get(face)).copied().unwrap_or(0) & 0x3) >= 2 {
            textured += 1;
        } else {
            untextured += 1;
        }
    }
    (total, untextured, textured)
}

fn source_face_has_kind(model: &Model, face: usize, kind: AlphaKind, pix: &Pix3DDraw) -> bool {
    if !(2..=254).contains(
        &model
            .face_alpha
            .as_ref()
            .and_then(|alpha| alpha.get(face))
            .copied()
            .unwrap_or(0),
    ) {
        return false;
    }
    let render_type = model
        .face_render_type
        .as_ref()
        .and_then(|types| types.get(face))
        .copied()
        .unwrap_or(0);
    if render_type == -1 {
        return false;
    }
    let textured = (render_type & 0x3) >= 2;
    match kind {
        AlphaKind::Untextured => !textured,
        AlphaKind::Textured => {
            if !textured {
                return false;
            }
            let id = model
                .face_colour
                .as_ref()
                .and_then(|colours| colours.get(face))
                .copied()
                .unwrap_or(-1);
            (0..50).contains(&id)
                && pix
                    .textures
                    .get(id as usize)
                    .is_some_and(|texture| texture.is_some())
        }
    }
}

fn source_near_face_count(model: &Model, camera: Camera) -> usize {
    let (Some(xs), Some(ys), Some(zs)) = (&model.point_x, &model.point_y, &model.point_z) else {
        return 0;
    };
    let (Some(as_), Some(bs), Some(cs)) = (
        &model.face_vertex_a,
        &model.face_vertex_b,
        &model.face_vertex_c,
    ) else {
        return 0;
    };
    let mut count = 0;
    let point_count = xs.len().min(ys.len()).min(zs.len());
    for face in 0..usize::try_from(model.num_faces).unwrap_or(0) {
        if model
            .face_render_type
            .as_ref()
            .and_then(|types| types.get(face))
            .copied()
            .unwrap_or(0)
            == -1
        {
            continue;
        }
        let Some([a, b, c]) = face_indices(as_, bs, cs, face, point_count) else {
            continue;
        };
        let z = [a, b, c].map(|index| view_point(model, index, camera)[2]);
        if z.iter().any(|&value| value < 50) && z.iter().any(|&value| value >= 50) {
            count += 1;
        }
    }
    count
}

fn face_indices(
    as_: &[i32],
    bs: &[i32],
    cs: &[i32],
    face: usize,
    point_count: usize,
) -> Option<[usize; 3]> {
    let indices = [*as_.get(face)?, *bs.get(face)?, *cs.get(face)?];
    let [a, b, c] = indices.map(|index| usize::try_from(index).ok());
    let (a, b, c) = (a?, b?, c?);
    (a < point_count && b < point_count && c < point_count).then_some([a, b, c])
}

fn view_point(model: &Model, point: usize, camera: Camera) -> [i32; 3] {
    let x0 = model.point_x.as_ref().unwrap()[point];
    let y0 = model.point_y.as_ref().unwrap()[point];
    let z0 = model.point_z.as_ref().unwrap()[point];
    let (mut x, mut y, mut z) = (x0, y0, z0);
    if camera.model_yaw != 0 {
        let sin = Pix3D::sin_table()[camera.model_yaw as usize];
        let cos = Pix3D::cos_table()[camera.model_yaw as usize];
        let temp = z.wrapping_mul(sin).wrapping_add(x.wrapping_mul(cos)) >> 16;
        z = z.wrapping_mul(cos).wrapping_sub(x.wrapping_mul(sin)) >> 16;
        x = temp;
    }
    x = x.wrapping_add(camera.relative_x);
    y = y.wrapping_add(camera.relative_y);
    z = z.wrapping_add(camera.relative_z);
    let temp = z
        .wrapping_mul(camera.sin_eye_yaw)
        .wrapping_add(x.wrapping_mul(camera.cos_eye_yaw))
        >> 16;
    z = z
        .wrapping_mul(camera.cos_eye_yaw)
        .wrapping_sub(x.wrapping_mul(camera.sin_eye_yaw))
        >> 16;
    x = temp;
    let temp = y
        .wrapping_mul(camera.cos_eye_pitch)
        .wrapping_sub(z.wrapping_mul(camera.sin_eye_pitch))
        >> 16;
    z = y
        .wrapping_mul(camera.sin_eye_pitch)
        .wrapping_add(z.wrapping_mul(camera.cos_eye_pitch))
        >> 16;
    y = temp;
    [x, y, z]
}

fn camera_for_target_face(model: &Model, face: usize, yaw: i32) -> Option<([i32; 3], Camera)> {
    let (Some(xs), Some(ys), Some(zs)) = (&model.point_x, &model.point_y, &model.point_z) else {
        return None;
    };
    let (Some(as_), Some(bs), Some(cs)) = (
        &model.face_vertex_a,
        &model.face_vertex_b,
        &model.face_vertex_c,
    ) else {
        return None;
    };
    let [a, b, c] = face_indices(as_, bs, cs, face, xs.len())?;
    let sin = Pix3D::sin_table()[yaw as usize];
    let cos = Pix3D::cos_table()[yaw as usize];
    let rotate = |point: usize| {
        let x = xs[point];
        let z = zs[point];
        let rx = if yaw == 0 {
            x
        } else {
            z.wrapping_mul(sin).wrapping_add(x.wrapping_mul(cos)) >> 16
        };
        let rz = if yaw == 0 {
            z
        } else {
            z.wrapping_mul(cos).wrapping_sub(x.wrapping_mul(sin)) >> 16
        };
        (rx, rz)
    };
    let target = [rotate(a), rotate(b), rotate(c)];
    let min_z = target.iter().map(|point| point.1).min()?;
    let max_z = target.iter().map(|point| point.1).max()?;
    if max_z.saturating_sub(min_z) < 2 {
        return None;
    }
    let relative_z = 49i32.checked_sub(min_z)?;
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    for point in 0..xs.len().min(ys.len()).min(zs.len()) {
        let (x, _) = rotate(point);
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(ys[point]);
        max_y = max_y.max(ys[point]);
    }
    if min_x == i32::MAX || min_y == i32::MAX {
        return None;
    }
    let center_x = i32::try_from((i64::from(min_x) + i64::from(max_x)) / 2).ok()?;
    let center_y = i32::try_from((i64::from(min_y) + i64::from(max_y)) / 2).ok()?;
    let relative_x = center_x.checked_neg()?;
    let relative_y = center_y.checked_neg()?;
    let camera = Camera::level(yaw, relative_x, relative_y, relative_z);
    let target_z = [a, b, c].map(|point| view_point(model, point, camera)[2]);
    if !target_z.iter().any(|&z| z < 50) || !target_z.iter().any(|&z| z > 50) {
        return None;
    }
    if !camera_fits_i16(model, camera) {
        return None;
    }
    Some((target_z, camera))
}

fn camera_fits_i16(model: &Model, camera: Camera) -> bool {
    let Some(xs) = model.point_x.as_ref() else {
        return false;
    };
    (0..xs.len()).all(|point| {
        let [x, y, z] = view_point(model, point, camera);
        i16::try_from(x).is_ok() && i16::try_from(y).is_ok() && i16::try_from(z).is_ok()
    })
}

fn find_translucent_case(
    archive: &[u8],
    pix: &mut Pix3DDraw,
    kind: AlphaKind,
) -> Result<ProofCase, ProofFailure> {
    let mut offset = 0usize;
    while offset < archive.len() {
        let (id, record, next) = parse_model_record(archive, offset)?;
        offset = next;
        Model::unpack(id, Some(record));
        let Some(mut model) = Model::load(id) else {
            Model::unload(id);
            continue;
        };
        Model::unload(id);
        let face_count = usize::try_from(model.num_faces).unwrap_or(0);
        if face_count == 0 || source_alpha_counts(&model).0 == 0 {
            continue;
        }
        model.calculate_normals(64, 768, -50, -10, -50, true);
        for yaw in MODEL_YAWS {
            let mut attempts = 0;
            for face in 0..face_count {
                if !source_face_has_kind(&model, face, kind, pix) {
                    continue;
                }
                let Some((target_z, camera)) = camera_for_target_face(&model, face, yaw) else {
                    continue;
                };
                let mesh = capture_model(&model, pix, camera)?;
                if mesh.vertices().len() > MAX_FIXTURE_VERTICES {
                    return Err(ProofFailure::FixtureTooLarge {
                        label: format!("model-{id}-{kind}"),
                        vertices: mesh.vertices().len(),
                    });
                }
                let metrics = mesh_metrics(&mesh);
                let emitted_target_kind = match kind {
                    AlphaKind::Untextured => metrics.alpha_untextured_triangles > 0,
                    AlphaKind::Textured => metrics.alpha_textured_triangles > 0,
                };
                if metrics.near_boundary_vertices > 0
                    && metrics.alpha_triangles > 0
                    && emitted_target_kind
                    && metrics.triangles >= 100
                    && mesh.retained_bytes() <= RETAINED_VERTEX_CAP
                {
                    return case_from_mesh(
                        &model,
                        CaseDescriptor {
                            label: format!("model-{id}-near-alpha-{kind}"),
                            model_id: id,
                            model_record_sha256: sha256_hex(record),
                            camera,
                            target_face: Some(face),
                            target_face_view_z: Some(target_z),
                        },
                        mesh,
                    );
                }
                attempts += 1;
                if attempts >= TARGET_FACE_TRIES_PER_YAW {
                    break;
                }
            }
        }
    }
    Err(ProofFailure::MissingAlphaModel(kind))
}

fn mesh_metrics(mesh: &SceneMesh) -> MeshMetrics {
    let vertices = mesh.vertices();
    let mut metrics = MeshMetrics {
        triangles: vertices.len() / 3,
        ..MeshMetrics::default()
    };
    for triangle in vertices.chunks_exact(3) {
        let alpha = (1..=254).contains(&triangle[0].alpha());
        let textured = triangle[0].texture_id_plus_one() != 0;
        if alpha {
            metrics.alpha_triangles += 1;
            if textured {
                metrics.alpha_textured_triangles += 1;
            } else {
                metrics.alpha_untextured_triangles += 1;
            }
        }
        let has_near_vertex = triangle.iter().any(|vertex| vertex.position()[2] == 50);
        if has_near_vertex {
            metrics.near_boundary_triangles += 1;
        }
        metrics.near_boundary_vertices += triangle
            .iter()
            .filter(|vertex| vertex.position()[2] == 50)
            .count();
    }
    metrics
}

fn compare_case(
    backend: &mut GpuBackend,
    oracle: &Legacy24Pipeline,
    pix: &mut Pix3DDraw,
    case: &ProofCase,
    mode: &str,
) -> Result<(), ProofFailure> {
    let mesh = &case.mesh;
    let vertex_count = mesh.vertices().len();
    if vertex_count > MAX_FIXTURE_VERTICES {
        return Err(ProofFailure::FixtureTooLarge {
            label: case.label.clone(),
            vertices: vertex_count,
        });
    }
    if vertex_count == 0 || vertex_count % 3 != 0 {
        return Err(ProofFailure::BudgetMode(format!(
            "{} has invalid captured triangle-list length {vertex_count}",
            case.label
        )));
    }
    let old_vertices = expand_legacy24(mesh, &case.label)?;
    let compact_live_bytes = vertex_count * std::mem::size_of::<GpuVertex>();
    let old_live_bytes = old_vertices.len() * std::mem::size_of::<Legacy24Vertex>();
    let old_capacity_bytes = old_vertices.capacity() * std::mem::size_of::<Legacy24Vertex>();
    if old_capacity_bytes > PEAK_VERTEX_LIMIT {
        return Err(ProofFailure::FixtureTooLarge {
            label: format!("{} legacy24 oracle", case.label),
            vertices: old_vertices.capacity(),
        });
    }

    let sampled_texture_ids = mesh.sampled_texture_ids();
    let _scene_lock = GPU_SCENE_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    {
        let mut assets = backend
            .context
            .assets
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        assets
            .ensure_model_textures(pix)
            .map_err(ProofFailure::Render)?;
        assets
            .stage_animated_model_textures(pix, &sampled_texture_ids)
            .map_err(ProofFailure::Render)?;
    }
    backend.render_scene(mesh).map_err(ProofFailure::Render)?;
    let compact = read_scene(backend);
    render_legacy24(backend, oracle, &old_vertices);
    let legacy = read_scene(backend);
    let compact_coverage = compact.iter().filter(|&&pixel| pixel != 0).count();
    let legacy_coverage = legacy.iter().filter(|&&pixel| pixel != 0).count();
    let mut differing_pixels = 0usize;
    let mut first = None;
    for (index, (&new, &old)) in compact.iter().zip(&legacy).enumerate() {
        if new != old {
            differing_pixels += 1;
            first.get_or_insert((index, new, old));
        }
    }
    let metrics = mesh_metrics(mesh);
    let backend_capacity = backend.vertex_buf_capacity;
    let backend_buffer_bytes = backend.vertex_buf.size() as usize;
    let retained_capture_bytes = mesh.retained_bytes();
    let backend_memory = crate::render::backend::RenderBackend::render_bytes(backend);
    if backend_capacity > RETAINED_VERTEX_CAP
        || backend_buffer_bytes != backend_capacity
        || compact_live_bytes > backend_capacity
    {
        return Err(ProofFailure::BudgetMode(format!(
            "{} compact buffer receipt violates the retained cap: live={compact_live_bytes}, buffer={backend_buffer_bytes}, capacity={backend_capacity}",
            case.label
        )));
    }
    if compact_live_bytes * 3 != old_live_bytes * 2 {
        return Err(ProofFailure::BudgetMode(format!(
            "{} payloads are not exact 16/24-byte records: compact={compact_live_bytes}, legacy24={old_live_bytes}",
            case.label
        )));
    }
    println!(
        "T44_COMPACT_PIXEL_CASE mode={mode} budget_receipt=backend-budget-scope-{mode} label={} model_id={} model_record_sha256={} camera={{model_yaw:{},eye_pitch_sin:{},eye_pitch_cos:{},eye_yaw_sin:{},eye_yaw_cos:{},relative_x:{},relative_y:{},relative_z:{}}} source_points={} source_faces={} source_alpha_faces={} source_alpha_untextured={} source_alpha_textured={} source_near_faces={} target_face={:?} target_face_view_z={:?} emitted_triangles={} emitted_alpha_triangles={} emitted_alpha_untextured={} emitted_alpha_textured={} near_boundary_faces={} near_boundary_vertices={} pixels={{compact:{},legacy24:{},differing:{}}} vertex_count={} render_bytes={{gpu_vertex_live:{},gpu_fixed_buffers:{},gpu_fixed_textures:{},backend_capture:{},total:{}}} bytes={{compact_payload16:{compact_live_bytes},legacy24_payload24:{old_live_bytes},compact_retained_capture:{retained_capture_bytes},compact_backend_capacity:{backend_capacity},compact_backend_buffer:{backend_buffer_bytes},legacy24_vec_capacity:{old_capacity_bytes},legacy24_reference_buffer_bytes:{old_live_bytes}}}",
        case.label,
        case.model_id,
        case.model_record_sha256,
        case.camera.model_yaw,
        case.camera.sin_eye_pitch,
        case.camera.cos_eye_pitch,
        case.camera.sin_eye_yaw,
        case.camera.cos_eye_yaw,
        case.camera.relative_x,
        case.camera.relative_y,
        case.camera.relative_z,
        case.source_points,
        case.source_faces,
        case.source_alpha_faces,
        case.source_alpha_untextured_faces,
        case.source_alpha_textured_faces,
        case.source_near_faces,
        case.target_face,
        case.target_face_view_z,
        metrics.triangles,
        metrics.alpha_triangles,
        metrics.alpha_untextured_triangles,
        metrics.alpha_textured_triangles,
        metrics.near_boundary_triangles,
        metrics.near_boundary_vertices,
        compact_coverage,
        legacy_coverage,
        differing_pixels,
        vertex_count,
        backend_memory.gpu_vertex_live(),
        backend_memory.gpu_fixed_buffers(),
        backend_memory.gpu_fixed_textures(),
        backend_memory.capture(),
        backend_memory.total(),
    );
    if compact_coverage < MIN_NON_BACKGROUND_PIXELS || legacy_coverage < MIN_NON_BACKGROUND_PIXELS {
        return Err(ProofFailure::InsufficientCoverage {
            label: case.label.clone(),
            pixels: compact_coverage.min(legacy_coverage),
        });
    }
    if differing_pixels != 0 {
        return Err(ProofFailure::PixelMismatch {
            label: case.label.clone(),
            differing_pixels,
            first,
        });
    }
    Ok(())
}

fn expand_legacy24(mesh: &SceneMesh, label: &str) -> Result<Vec<Legacy24Vertex>, ProofFailure> {
    let source = mesh.vertices();
    if source.len() > MAX_FIXTURE_VERTICES {
        return Err(ProofFailure::FixtureTooLarge {
            label: label.to_owned(),
            vertices: source.len(),
        });
    }
    let mut result = Vec::new();
    result.try_reserve_exact(source.len()).map_err(|error| {
        ProofFailure::BudgetMode(format!("legacy24 fixture allocation failed: {error}"))
    })?;
    for vertex in source {
        let [x, y, z] = vertex.position();
        let [u, v] = vertex.texture_uv();
        let alpha = u32::from(vertex.alpha());
        let hclip = if vertex.polygon_hclip() { 1u32 } else { 0 };
        result.push(Legacy24Vertex {
            x: x as f32,
            y: y as f32,
            z: z as f32,
            abhsl: (alpha << 24) | (hclip << 16) | u32::from(vertex.shade()),
            uv_tex: (u32::from(u as u16) << 16) | u32::from(vertex.texture_id_plus_one()),
            v: u32::from(v as u16),
        });
    }
    Ok(result)
}

impl Legacy24Pipeline {
    fn new(context: &GpuContext) -> Self {
        let device = &context.device;
        let assets = context
            .assets
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let model_layout = assets.model_bind_group_layout.clone();
        drop(assets);
        let scene_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("T4.4 baseline 23ba29e scene data layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(16),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(3 * 24),
                    },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("T4.4 baseline 23ba29e pipeline layout"),
            bind_group_layouts: &[Some(&model_layout), Some(&scene_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("T4.4 baseline 23ba29e legacy24 WGSL"),
            source: wgpu::ShaderSource::Wgsl(include_str!("gpu_vertex_legacy24.wgsl").into()),
        });
        let attributes = wgpu::vertex_attr_array![
            0 => Float32x3,
            1 => Uint32,
            2 => Uint32,
            3 => Uint32,
        ];
        assert_eq!(
            attributes.map(|attribute| attribute.offset),
            [0, 12, 16, 20]
        );
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("T4.4 baseline 23ba29e legacy24 reference"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 24,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                }],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::OVER,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            pipeline,
            scene_layout,
        }
    }
}

fn render_legacy24(backend: &GpuBackend, oracle: &Legacy24Pipeline, vertices: &[Legacy24Vertex]) {
    assert!(!vertices.is_empty());
    let device = &backend.context.device;
    let queue = &backend.context.queue;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("T4.4 fixture-only baseline 24-byte vertices"),
        size: (vertices.len() * std::mem::size_of::<Legacy24Vertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX
            | wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, bytemuck::cast_slice(vertices));
    let brightness = [Pix3D::colour_brightness() as f32, 0.0f32, 0.0f32, 0.0f32];
    queue.write_buffer(
        &backend.brightness_buf,
        0,
        bytemuck::cast_slice(&brightness),
    );
    let scene_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("T4.4 baseline legacy24 scene group"),
        layout: &oracle.scene_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: backend.brightness_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: buffer.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("T4.4 baseline legacy24 encoder"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("T4.4 baseline legacy24 scene pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &backend.scene_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&oracle.pipeline);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.set_bind_group(0, &backend.model_bind_group, &[]);
        pass.set_bind_group(1, &scene_group, &[]);
        pass.draw(
            0..u32::try_from(vertices.len()).expect("bounded legacy24 vertex count"),
            0..1,
        );
    }
    queue.submit(Some(encoder.finish()));
}

fn read_scene(backend: &GpuBackend) -> Vec<i32> {
    TextureHandle::untracked(
        backend.context.device.clone(),
        backend.context.queue.clone(),
        backend.scene_view.clone(),
        SCENE_W,
        SCENE_H,
    )
    .read_back()
}

fn check_vertex_preflight(backend: &mut GpuBackend) -> Result<(), ProofFailure> {
    let old_capacity = backend.vertex_buf_capacity;
    let old_buffer_size = backend.vertex_buf.size() as usize;
    let last_image = read_scene(backend);
    let max_plus_one = GpuBackend::MAX_VERTEX_RETAINED_BYTES
        .checked_add(1)
        .ok_or_else(|| ProofFailure::VertexPreflight("128 MiB cap overflow".into()))?;
    let error = backend
        .ensure_vertex_buffer(max_plus_one)
        .expect_err("MAX_VERTEX_RETAINED_BYTES + 1 must be refused before allocation");
    if error.resource() != RenderResource::Vertex
        || backend.vertex_buf_capacity != old_capacity
        || backend.vertex_buf.size() as usize != old_buffer_size
        || read_scene(backend) != last_image
    {
        return Err(ProofFailure::VertexPreflight(format!(
            "MAX+1 did not fail closed: error={error}, capacity {}->{}, buffer {}->{}, image_unchanged={}",
            old_capacity,
            backend.vertex_buf_capacity,
            old_buffer_size,
            backend.vertex_buf.size(),
            read_scene(backend) == last_image
        )));
    }
    let effective_maximum = GpuBackend::MAX_VERTEX_RETAINED_BYTES.min(
        backend
            .context
            .device
            .limits()
            .max_storage_buffer_binding_size as usize,
    );
    if error.maximum() != effective_maximum {
        return Err(ProofFailure::VertexPreflight(format!(
            "MAX+1 reported maximum {}, expected device-limited retained cap {effective_maximum}",
            error.maximum()
        )));
    }
    println!(
        "T44_VERTEX_PREFLIGHT retained_request={} requested_capacity={} error={{resource:{},requested:{},maximum:{}}} buffer_capacity={} buffer_bytes={} last_image_unchanged=true",
        max_plus_one,
        error.requested(),
        error.resource(),
        error.requested(),
        error.maximum(),
        backend.vertex_buf_capacity,
        backend.vertex_buf.size(),
    );

    let capacity = backend.vertex_buf_capacity;
    if capacity == 0 || !capacity.is_power_of_two() {
        return Err(ProofFailure::VertexPreflight(format!(
            "current backend capacity {capacity} is not a nonzero power of two"
        )));
    }
    let doubled = capacity
        .checked_mul(2)
        .ok_or_else(|| ProofFailure::VertexPreflight("2*C overflows usize".into()))?;
    let three = capacity
        .checked_mul(3)
        .ok_or_else(|| ProofFailure::VertexPreflight("3*C overflows usize".into()))?;
    if doubled > RETAINED_VERTEX_CAP || three > PEAK_VERTEX_LIMIT {
        return Err(ProofFailure::VertexPreflight(format!(
            "C={capacity} is too large for bounded growth proof (2*C={doubled}, 3*C={three})"
        )));
    }
    let buffer_before = backend.vertex_buf.size() as usize;

    let rejected_budget = fixture_budget(three - 1);
    let rejected_vertex = rejected_budget.for_resource(RenderResource::Vertex).clone();
    let rejected_scope = rejected_budget.scope();
    let rejected = backend.ensure_vertex_buffer(capacity + 1);
    drop(rejected_scope);
    let rejected_error = rejected.expect_err("peak 3*C-1 must refuse C+1 growth");
    if rejected_error.resource() != RenderResource::Vertex
        || rejected_error.requested() != three
        || rejected_error.maximum() != three - 1
        || backend.vertex_buf_capacity != capacity
        || backend.vertex_buf.size() as usize != buffer_before
        || rejected_vertex.live() != capacity
        || rejected_vertex.peak() != capacity
    {
        return Err(ProofFailure::VertexPreflight(format!(
            "3*C-1 growth rejection mismatched: error={rejected_error}, capacity={}, buffer={}, charge_live={}, charge_peak={}, C={capacity}",
            backend.vertex_buf_capacity,
            backend.vertex_buf.size(),
            rejected_vertex.live(),
            rejected_vertex.peak()
        )));
    }
    println!(
        "T44_VERTEX_GROWTH mode=peak-3C-1 C={capacity} request={} expected_capacity={} error={{resource:{},requested:{},maximum:{}}} retained_capacity={} buffer_bytes={} charge_live={} charge_peak={} allocated=false",
        capacity + 1,
        doubled,
        rejected_error.resource(),
        rejected_error.requested(),
        rejected_error.maximum(),
        backend.vertex_buf_capacity,
        backend.vertex_buf.size(),
        rejected_vertex.live(),
        rejected_vertex.peak(),
    );

    let admitted_budget = fixture_budget(three);
    let admitted_vertex = admitted_budget.for_resource(RenderResource::Vertex).clone();
    let admitted_scope = admitted_budget.scope();
    backend
        .ensure_vertex_buffer(capacity + 1)
        .map_err(|error| {
            ProofFailure::VertexPreflight(format!("3*C growth was not admitted: {error}"))
        })?;
    drop(admitted_scope);
    if backend.vertex_buf_capacity != doubled
        || backend.vertex_buf.size() as usize != doubled
        || admitted_vertex.live() != doubled
        || admitted_vertex.peak() != three
    {
        return Err(ProofFailure::VertexPreflight(format!(
            "3*C growth receipt mismatched: capacity={}, buffer={}, charge_live={}, charge_peak={}, expected_capacity={doubled}, expected_peak={three}",
            backend.vertex_buf_capacity,
            backend.vertex_buf.size(),
            admitted_vertex.live(),
            admitted_vertex.peak(),
        )));
    }
    println!(
        "T44_VERTEX_GROWTH mode=peak-3C C={capacity} request={} new_capacity={} buffer_bytes={} charge_live={} charge_peak={} retained_cap={}MiB",
        capacity + 1,
        backend.vertex_buf_capacity,
        backend.vertex_buf.size(),
        admitted_vertex.live(),
        admitted_vertex.peak(),
        RETAINED_VERTEX_CAP / MIB,
    );
    Ok(())
}
