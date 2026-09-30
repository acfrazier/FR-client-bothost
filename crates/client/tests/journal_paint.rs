use std::sync::Arc;

use client::client::{Client, ClientConfig};
use client::config::if_type::{ComponentType, IfType, IfTypeMut};
use client::graphics::Pix8;
use client::io::ClientProt;
use client::render::backend::FrameOutput;
use client::render::media::Media;
use client::render::Renderer;

const MAIN_ROOT: i32 = 90;
const MAIN_RECT: i32 = 91;
const SIDE_ROOT: i32 = 100;
const SIDE_RECT: i32 = 101;
const MAIN_COLOUR: i32 = 0x0012_3456;
const SIDE_COLOUR: i32 = 0x0022_4466;
const SIDE_RESTORED_COLOUR: i32 = 0x0066_4422;
const TOP_TAB_COLOUR: i32 = 0x0000_aa55;
const BOTTOM_TAB_COLOUR: i32 = 0x00aa_5500;
const TOP_TAB_RESTORED_COLOUR: i32 = 0x0055_00aa;
const BOTTOM_TAB_RESTORED_COLOUR: i32 = 0x00aa_0055;

fn client() -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

fn rect_layer(
    client: &mut Client,
    root_id: i32,
    rect_id: i32,
    width: i32,
    height: i32,
    colour: i32,
) {
    client.set_iface(
        root_id as usize,
        IfType {
            id: root_id,
            r#type: ComponentType::TYPE_LAYER,
            width,
            height,
            children: Some(vec![rect_id]),
            child_x: Some(vec![0]),
            child_y: Some(vec![0]),
            ..Default::default()
        },
    );
    client.set_iface(
        rect_id as usize,
        IfType {
            id: rect_id,
            r#type: ComponentType::TYPE_RECT,
            width,
            height,
            fill: true,
            ..Default::default()
        },
    );
    client.set_iface_mut(
        rect_id as usize,
        IfTypeMut {
            colour,
            ..Default::default()
        },
    );
}

fn solid_sprite(width: i32, height: i32, colour: i32) -> Pix8 {
    let mut sprite = Pix8::new(width, height, vec![0, colour]);
    sprite.data.fill(1);
    sprite
}

fn install_tab_media(renderer: &mut Renderer, top_colour: i32, bottom_colour: i32) {
    let mut media = Media::empty();
    media.backhmid1 = Some(solid_sprite(249, 45, top_colour));
    media.backbase2 = Some(solid_sprite(269, 37, bottom_colour));
    renderer.media = Arc::new(media);
}

fn frame_is_cpu(frame: FrameOutput) {
    assert!(
        matches!(frame, FrameOutput::PixMap(_)),
        "journal paint test requires CPU pixels"
    );
}

#[test]
fn hidden_journal_suppresses_modal_pixels_and_restores_them() {
    Renderer::set_prefer_gpu(false);
    let mut renderer = Renderer::new(false);
    let mut client = client();
    client.ingame = true;
    client.scene_state = 2;
    rect_layer(&mut client, MAIN_ROOT, MAIN_RECT, 512, 334, MAIN_COLOUR);
    client.main_modal_id = MAIN_ROOT;
    client.active_icon = 5;
    client.side_modal_id = 17;

    frame_is_cpu(renderer.game_draw(&mut client));
    let modal_pixel = (14 * renderer.draw_area.width + 14) as usize;
    assert_eq!(renderer.draw_area.pixels[modal_pixel], MAIN_COLOUR);
    assert!(!client.journal_paint_hidden());
    let ids_before = (
        client.main_modal_id,
        client.side_modal_id,
        client.active_icon,
    );

    // A freeze entered while the modal is visible must retain that last FBO;
    // the hidden flag only gates the live scene's modal raster.
    client.scene_state = 1;
    client.set_journal_paint_hidden(true);
    frame_is_cpu(renderer.game_draw(&mut client));
    assert_eq!(
        renderer.draw_area.pixels[modal_pixel], MAIN_COLOUR,
        "CPU scene_state==1 must keep the last-FBO modal pixels"
    );

    client.scene_state = 2;
    frame_is_cpu(renderer.game_draw(&mut client));
    assert_ne!(
        renderer.draw_area.pixels[modal_pixel], MAIN_COLOUR,
        "hidden journal paint must remove the main modal from a live scene"
    );
    assert_eq!(
        (
            client.main_modal_id,
            client.side_modal_id,
            client.active_icon
        ),
        ids_before,
        "paint suppression must not mutate modal or tab state"
    );

    client.set_journal_paint_hidden(false);
    assert!(!client.journal_paint_hidden());
    frame_is_cpu(renderer.game_draw(&mut client));
    assert_eq!(renderer.draw_area.pixels[modal_pixel], MAIN_COLOUR);
}

#[test]
fn hidden_journal_retains_side_and_tab_pixels_and_preserves_clickside_packet() {
    Renderer::set_prefer_gpu(false);
    let mut renderer = Renderer::new(false);
    let mut client = client();
    client.ingame = true;
    client.scene_state = 0;
    rect_layer(&mut client, SIDE_ROOT, SIDE_RECT, 190, 261, SIDE_COLOUR);
    client.side_modal_id = SIDE_ROOT;
    client.active_icon = 3;

    frame_is_cpu(renderer.game_draw(&mut client));
    install_tab_media(&mut renderer, TOP_TAB_COLOUR, BOTTOM_TAB_COLOUR);
    client.redraw_frame = false;
    client.redraw_side = true;
    client.redraw_icons = true;
    frame_is_cpu(renderer.game_draw(&mut client));

    let side_pixel = (215 * renderer.draw_area.width + 563) as usize;
    let top_tab_pixel = (165 * renderer.draw_area.width + 521) as usize;
    let bottom_tab_pixel = (471 * renderer.draw_area.width + 501) as usize;
    let before = [
        renderer.draw_area.pixels[side_pixel],
        renderer.draw_area.pixels[top_tab_pixel],
        renderer.draw_area.pixels[bottom_tab_pixel],
    ];
    assert_eq!(before, [SIDE_COLOUR, TOP_TAB_COLOUR, BOTTOM_TAB_COLOUR]);

    client.overlay_mut(SIDE_RECT as usize).unwrap().colour = SIDE_RESTORED_COLOUR;
    install_tab_media(
        &mut renderer,
        TOP_TAB_RESTORED_COLOUR,
        BOTTOM_TAB_RESTORED_COLOUR,
    );
    client.set_journal_paint_hidden(true);
    client.redraw_side = true;
    client.redraw_icons = true;
    client.tut_flash_icon = client.active_icon;
    frame_is_cpu(renderer.game_draw(&mut client));
    assert_eq!(
        [
            renderer.draw_area.pixels[side_pixel],
            renderer.draw_area.pixels[top_tab_pixel],
            renderer.draw_area.pixels[bottom_tab_pixel],
        ],
        before,
        "hidden paint must retain pre-read side and both tab-bar surfaces"
    );
    assert_eq!(
        client.tut_flash_icon, -1,
        "draw_icons still consumes the flash edge"
    );
    assert_eq!(client.out.data()[0], ClientProt::TUT_CLICKSIDE.id as u8);
    assert_eq!(client.out.data()[1], client.active_icon as u8);

    client.set_journal_paint_hidden(false);
    assert!(client.redraw_side && client.redraw_icons);
    frame_is_cpu(renderer.game_draw(&mut client));
    assert_eq!(renderer.draw_area.pixels[side_pixel], SIDE_RESTORED_COLOUR);
    assert_eq!(
        renderer.draw_area.pixels[top_tab_pixel],
        TOP_TAB_RESTORED_COLOUR
    );
    assert_eq!(
        renderer.draw_area.pixels[bottom_tab_pixel],
        BOTTOM_TAB_RESTORED_COLOUR
    );
}

#[test]
fn journal_read_open_paint_close_has_identical_r289_packets_when_hidden() {
    use client::client::mini_menu_action::MiniMenuAction;
    use client::io::{ClientProt289, ClientRevision};

    fn read(hidden: bool) -> (Vec<u8>, (i32, i32)) {
        let mut client = Client::new_with_revision(
            ClientConfig {
                host: "127.0.0.1".into(),
                port: 43594,
                cache_dir: "/tmp".into(),
                members: true,
                lowmem: false,
            },
            ClientRevision::R289,
        );
        let mut renderer = Renderer::new_prefer(false, false);
        client.ingame = true;
        client.scene_state = 2;
        client.active_icon = 3;
        rect_layer(&mut client, MAIN_ROOT, MAIN_RECT, 512, 334, MAIN_COLOUR);
        frame_is_cpu(renderer.game_draw(&mut client));

        client.set_journal_paint_hidden(hidden);
        client.menu_action[0] = MiniMenuAction::IF_BUTTON;
        client.menu_param_c[0] = 42;
        client.doAction(0);
        // The server's ordinary journal-open pair; painting must not rewrite it.
        client.main_modal_id = MAIN_ROOT;
        client.side_modal_id = SIDE_ROOT;
        client.redraw_side = true;
        client.redraw_icons = true;
        client.tut_flash_icon = client.active_icon;
        frame_is_cpu(renderer.game_draw(&mut client));
        client.menu_action[0] = MiniMenuAction::CLOSE_BUTTON;
        client.doAction(0);
        frame_is_cpu(renderer.game_draw(&mut client));
        client.set_journal_paint_hidden(false);
        (
            client.out.data()[..client.out.pos].to_vec(),
            (client.main_modal_id, client.side_modal_id),
        )
    }

    let visible = read(false);
    let quiet = read(true);
    assert_eq!(
        quiet, visible,
        "open/paint/close must preserve packets and modal state"
    );
    assert_eq!(
        quiet.0,
        [
            ClientProt289::IF_BUTTON.id as u8,
            0,
            42,
            ClientProt289::TUT_CLICKSIDE.id as u8,
            3,
            ClientProt289::CLOSE_MODAL.id as u8,
        ],
        "the packet trace contains the same row click, paint-time side acknowledgement, and close"
    );
    assert_eq!(quiet.1, (-1, -1));
}
