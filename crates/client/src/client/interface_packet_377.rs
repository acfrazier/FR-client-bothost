//! Pure revision-377 interface/modal/tutorial packet decoding.
//!
//! Wire order and state meaning follow Client.java:7268-8570. In particular,
//! opcode 18 is not an angle tuple: Java packs its two words into the raw
//! `Component.field700` int. Keep that Java word intact for the later apply
//! boundary.
//!
//! Engine codec cross-checks: the field order and byte transforms match the
//! corresponding `If*Encoder.ts` files. Java reads TUT_OPEN's g2b_alt1 and
//! IF_OPENOVERLAY's g2b as signed component IDs even though TutOpenEncoder
//! uses a plain numeric component and IfOpenOverlayEncoder writes p2; only -1
//! is the no-interface sentinel, and other negative IDs fail resource checks.
//! IfSetRotationEncoder names opcode 18's words x/y angle speeds, but Java
//! stores their packed int rather than a new angle model. IF_OPENFULL's Java
//! g2_alt1 overlay is unsigned and is not normalized from 65535 to a sentinel.
//! The engine declares P_NAMEDIALOG length 0 (`ServerGameProt.ts:68`) but has
//! no dedicated codec file; Java handles opcode 6 as a bodyless prompt.

use super::{
    require_reference, PacketContext377, PacketError377, PacketErrorKind377, PacketReader377,
    ReferenceKind377,
};

/// Server packets decoded by this interface/modal/tutorial boundary.
pub const ALL_OPCODES_377: &[u8] = &[
    166, 186, 216, 162, 109, 158, 218, 6, 29, 50, 82, 128, 58, 252, 255, 159, 246, 21, 2, 10, 238,
    253, 18, 232, 200,
];

/// Decoded command meaning for one revision-377 interface packet.
///
/// Values remain Java `int`s. Fields that preserve a wire sentinel or a
/// signed Java read are intentionally not narrowed or converted to unsigned
/// Rust types. Text borrows the packet's bytes, excluding its LF terminator.
#[derive(Debug, PartialEq, Eq)]
pub enum InterfaceOperation377<'a> {
    /// IF_SETPOSITION (166), Client.java:7268-7275; wire order is signed y,
    /// signed x, component, while Java assigns the first coordinate to y.
    SetPosition { component: i32, x: i32, y: i32 },
    /// IF_SETANGLE (186), Client.java:7280-7288.
    SetAngle {
        component: i32,
        xan: i32,
        yan: i32,
        zoom: i32,
    },
    /// IF_SETMODEL (216), Client.java:7293-7298.
    SetModel { component: i32, model: i32 },
    /// IF_SETNPCHEAD (162), Client.java:7365-7370.
    SetNpcHead { component: i32, npc: i32 },
    /// IF_OPENCHAT (109), Client.java:7375-7404.
    OpenChat { component: i32 },
    /// TUT_OPEN (158), Client.java:7438-7445. The signed g2b_alt1 value keeps
    /// Java's -1 no-interface sentinel; other values require a component.
    TutOpen { component: i32 },
    /// IF_SETCOLOUR (218), Client.java:7450-7458. `rgb555` is the raw
    /// transformed 16-bit packet word; Java expands it to its stored color
    /// during application.
    SetColour { component: i32, rgb555: i32 },
    /// P_NAMEDIALOG (6), Client.java:7478-7485.
    NameDialog,
    /// IF_CLOSE (29), Client.java:7591-7623.
    Close,
    /// IF_OPENOVERLAY (50), Client.java:7694-7705. Java reads signed g2b;
    /// only -1 is the no-interface sentinel.
    OpenOverlay { component: i32 },
    /// IF_SETHIDE (82), Client.java:7708-7714; only byte value 1 means true.
    SetHide { component: i32, hidden: bool },
    /// IF_OPENMAIN_SIDE (128), Client.java:7741-7775.
    OpenMainSide { main: i32, side: i32 },
    /// P_COUNTDIALOG (58), Client.java:7865-7872.
    CountDialog,
    /// IF_SETTAB_ACTIVE (252), Client.java:7875-7880.
    SetTabActive { tab: i32 },
    /// IF_SETPLAYERHEAD (255), Client.java:7905-7915.
    SetPlayerHead { component: i32 },
    /// IF_OPENMAIN (159), Client.java:7973-8007.
    OpenMain { component: i32 },
    /// IF_OPENSIDE (246), Client.java:8010-8044.
    OpenSide { component: i32 },
    /// IF_SETOBJECT (21), Client.java:8329-8346. Object 65535 is Java's
    /// no-model sentinel; its divisor is unused in that branch.
    SetObject {
        component: i32,
        object: i32,
        divisor: i32,
    },
    /// IF_SETANIM (2), Client.java:8366-8376; sequence -1 means no animation.
    SetAnim { component: i32, sequence: i32 },
    /// IF_SETTAB (10), Client.java:8397-8410; component 65535 is explicitly
    /// normalized to Java's -1 tab-interface sentinel.
    SetTab { tab: i32, component: i32 },
    /// TUT_FLASH (238), Client.java:8426-8438. Java accepts every byte value;
    /// values outside visible tabs are retained instead of rejected.
    TutFlash { tab: i32 },
    /// IF_OPENFULL (253), Client.java:8467-8498. Both words are unsigned in
    /// Java; preserve them as-is rather than converting 65535 to -1.
    OpenFull { overlay: i32, main: i32 },
    /// IF_SETROTATION (18), Client.java:8509-8515. `packed` is exactly
    /// `(g2 << 16) + g2_alt1`, Java's raw Component.field700 value.
    SetRotation { component: i32, packed: i32 },
    /// IF_SETTEXT (232), Client.java:8540-8549.
    SetText { component: i32, text: &'a [u8] },
    /// IF_SETSCROLLPOS (200), Client.java:8553-8568.
    SetScrollPos { component: i32, position: i32 },
}

impl<'a> InterfaceOperation377<'a> {
    /// Decode one packet body. Unknown opcodes are left for another packet
    /// family; the packet root enforces exact body consumption.
    pub fn decode(opcode: u8, p: &mut PacketReader377<'a>) -> Result<Option<Self>, PacketError377> {
        let operation = match opcode {
            166 => {
                let y = p.g2b_alt1()?;
                let x = p.g2b_alt1()?;
                let component = p.g2()?;
                Self::SetPosition { component, x, y }
            }
            186 => {
                let xan = p.g2_alt2()?;
                let component = p.g2_alt3()?;
                let zoom = p.g2_alt2()?;
                let yan = p.g2_alt1()?;
                Self::SetAngle {
                    component,
                    xan,
                    yan,
                    zoom,
                }
            }
            216 => {
                let model = p.g2_alt3()?;
                let component = p.g2_alt3()?;
                Self::SetModel { component, model }
            }
            162 => {
                let npc = p.g2_alt2()?;
                let component = p.g2_alt1()?;
                Self::SetNpcHead { component, npc }
            }
            109 => Self::OpenChat { component: p.g2()? },
            158 => Self::TutOpen {
                component: p.g2b_alt1()?,
            },
            218 => Self::SetColour {
                component: p.g2()?,
                rgb555: p.g2_alt2()?,
            },
            6 => Self::NameDialog,
            29 => Self::Close,
            50 => Self::OpenOverlay {
                component: p.g2b()?,
            },
            82 => {
                let hidden = p.g1()? == 1;
                let component = p.g2()?;
                Self::SetHide { component, hidden }
            }
            128 => Self::OpenMainSide {
                main: p.g2_alt2()?,
                side: p.g2_alt3()?,
            },
            58 => Self::CountDialog,
            252 => {
                let tab = p.g1_alt2()?;
                if tab > 14 {
                    return Err(p.invalid("tab_index"));
                }
                Self::SetTabActive { tab }
            }
            255 => Self::SetPlayerHead {
                component: p.g2_alt3()?,
            },
            159 => Self::OpenMain {
                component: p.g2_alt3()?,
            },
            246 => Self::OpenSide {
                component: p.g2_alt3()?,
            },
            21 => {
                let divisor = p.g2()?;
                let object = p.g2_alt1()?;
                let component = p.g2_alt3()?;
                if object != 0xFFFF && divisor == 0 {
                    return Err(p.invalid("object_divisor"));
                }
                Self::SetObject {
                    component,
                    object,
                    divisor,
                }
            }
            2 => Self::SetAnim {
                component: p.g2_alt3()?,
                sequence: p.g2b_alt2()?,
            },
            10 => {
                let tab = p.g1_alt3()?;
                let component = p.g2_alt2()?;
                if tab > 14 {
                    return Err(p.invalid("tab_index"));
                }
                let component = if component == 0xFFFF { -1 } else { component };
                Self::SetTab { tab, component }
            }
            238 => Self::TutFlash { tab: p.g1()? },
            253 => Self::OpenFull {
                overlay: p.g2_alt1()?,
                main: p.g2_alt2()?,
            },
            18 => {
                let high = p.g2()?;
                let component = p.g2_alt2()?;
                let low = p.g2_alt1()?;
                // Keep the exact Java expression and 32-bit representation.
                let packed = (high << 16) + low;
                Self::SetRotation { component, packed }
            }
            232 => Self::SetText {
                component: p.g2_alt3()?,
                text: p.gjstr()?,
            },
            200 => Self::SetScrollPos {
                component: p.g2()?,
                position: p.g2_alt3()?,
            },
            _ => return Ok(None),
        };
        Ok(Some(operation))
    }

    /// Validate resource references after decoding and before a packet can be
    /// handed off. This only reads the supplied context.
    pub fn validate(&self, context: &impl PacketContext377) -> Result<(), PacketErrorKind377> {
        match self {
            Self::SetPosition { component, .. }
            | Self::SetAngle { component, .. }
            | Self::SetColour { component, .. }
            | Self::SetHide { component, .. }
            | Self::SetPlayerHead { component }
            | Self::SetText { component, .. }
            | Self::SetScrollPos { component, .. }
            | Self::SetRotation { component, .. } => {
                require_reference(context, ReferenceKind377::Component, *component)
            }
            Self::SetModel { component, model } => {
                require_reference(context, ReferenceKind377::Component, *component)?;
                require_reference(context, ReferenceKind377::Model, *model)
            }
            Self::SetNpcHead { component, npc } => {
                require_reference(context, ReferenceKind377::Component, *component)?;
                require_reference(context, ReferenceKind377::Npc, *npc)
            }
            Self::OpenChat { component }
            | Self::OpenMain { component }
            | Self::OpenSide { component } => {
                require_reference(context, ReferenceKind377::Component, *component)
            }
            Self::TutOpen { component } | Self::OpenOverlay { component } => {
                if *component != -1 {
                    require_reference(context, ReferenceKind377::Component, *component)?;
                }
                Ok(())
            }
            Self::OpenMainSide { main, side } => {
                require_reference(context, ReferenceKind377::Component, *main)?;
                require_reference(context, ReferenceKind377::Component, *side)
            }
            Self::SetObject {
                component, object, ..
            } => {
                require_reference(context, ReferenceKind377::Component, *component)?;
                if *object != 0xFFFF {
                    require_reference(context, ReferenceKind377::Object, *object)?;
                }
                Ok(())
            }
            Self::SetAnim {
                component,
                sequence,
            } => {
                require_reference(context, ReferenceKind377::Component, *component)?;
                if *sequence != -1 {
                    require_reference(context, ReferenceKind377::Sequence, *sequence)?;
                }
                Ok(())
            }
            Self::SetTab { component, .. } => {
                if *component != -1 {
                    require_reference(context, ReferenceKind377::Component, *component)?;
                }
                Ok(())
            }
            Self::OpenFull { overlay, main } => {
                require_reference(context, ReferenceKind377::Component, *overlay)?;
                require_reference(context, ReferenceKind377::Component, *main)
            }
            Self::NameDialog
            | Self::Close
            | Self::CountDialog
            | Self::SetTabActive { .. }
            | Self::TutFlash { .. } => Ok(()),
        }
    }
}
