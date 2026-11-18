use pumpkin_util::text::TextComponent;

use crate::ser::NetworkWriteExt;
use pumpkin_util::version::JavaMinecraftVersion;

pub struct DialogNBT<'a>(pub DialogNBTSource<'a>);

impl<'a> DialogNBT<'a> {
    #[must_use]
    pub const fn from_dialog(dialog: &'a Dialog) -> Self {
        Self(DialogNBTSource::Struct(dialog))
    }

    #[must_use]
    pub const fn from_nbt(compound: &'a pumpkin_nbt::compound::NbtCompound) -> Self {
        Self(DialogNBTSource::Nbt(compound))
    }

    pub fn write_packet_data(
        &self,
        mut write: impl std::io::Write,
        version: &JavaMinecraftVersion,
    ) -> Result<(), crate::ser::WritingError> {
        let compound = match &self.0 {
            DialogNBTSource::Struct(dialog) => dialog.to_nbt(version),
            DialogNBTSource::Nbt(nbt) => (*nbt).clone(),
        };
        let tag = pumpkin_nbt::tag::NbtTag::Compound(compound);
        write.write_nbt_with_version(Some(&tag), version)?;
        Ok(())
    }
}

pub enum DialogNBTSource<'a> {
    Struct(&'a Dialog),
    Nbt(&'a pumpkin_nbt::compound::NbtCompound),
}

#[derive(Clone, Debug)]
pub struct Dialog {
    pub r#type: String,
    pub title: TextComponent,
    pub body: Vec<DialogBody>,
    pub inputs: Vec<DialogInput>,
    pub buttons: Vec<ActionButton>,
    pub links: Vec<DialogLink>,
    pub exit_action: Option<DialogAction>,
    pub after_action: Option<String>,
    pub can_close_with_escape: bool,
    pub external_title: Option<TextComponent>,
}

impl Dialog {
    /// The dialog in vanilla's network form (`net.minecraft.server.dialog` codecs).
    /// Inputs and item bodies are left out: vanilla needs an input key and a full item stack.
    #[must_use]
    pub fn to_nbt(&self, version: &JavaMinecraftVersion) -> pumpkin_nbt::compound::NbtCompound {
        use pumpkin_nbt::{compound::NbtCompound, tag::NbtTag};
        let text = |t: &TextComponent| t.to_nbt_tag_for_version(version);
        let button = |b: &ActionButton| {
            let mut nbt = NbtCompound::new();
            nbt.put("label", text(&b.text));
            if let Some(tooltip) = &b.tooltip {
                nbt.put("tooltip", text(tooltip));
            }
            if let Some(width) = b.width {
                nbt.put_int("width", width.clamp(1, 1024).cast_signed());
            }
            nbt.put_compound("action", b.action.to_nbt());
            NbtTag::Compound(nbt)
        };

        let mut nbt = NbtCompound::new();
        nbt.put_string("type", self.r#type.clone());
        nbt.put("title", text(&self.title));
        if let Some(external_title) = &self.external_title {
            nbt.put("external_title", text(external_title));
        }
        let body = self
            .body
            .iter()
            .filter_map(|b| match b {
                DialogBody::PlainMessage { contents } => {
                    let mut body = NbtCompound::new();
                    body.put_string("type", "minecraft:plain_message".into());
                    body.put("contents", text(contents));
                    Some(NbtTag::Compound(body))
                }
                DialogBody::Item { .. } => None,
            })
            .collect();
        nbt.put_list("body", body);
        nbt.put_bool("can_close_with_escape", self.can_close_with_escape);
        // Vanilla's DialogAction: close, none or wait_for_response.
        if let Some(after) = self.after_action.as_deref() {
            let after = match after {
                "none" | "peek" => "none",
                "wait_for_response" => "wait_for_response",
                _ => "close",
            };
            nbt.put_string("after_action", after.into());
        }
        match self.r#type.as_str() {
            "minecraft:multi_action" => {
                nbt.put_list("actions", self.buttons.iter().map(button).collect());
            }
            "minecraft:notice" => {
                if let Some(action) = self.buttons.first() {
                    nbt.put("action", button(action));
                }
            }
            "minecraft:confirmation" => {
                if let [yes, no, ..] = self.buttons.as_slice() {
                    nbt.put("yes", button(yes));
                    nbt.put("no", button(no));
                }
            }
            _ => {}
        }
        nbt
    }
}

impl DialogAction {
    fn to_nbt(&self) -> pumpkin_nbt::compound::NbtCompound {
        let mut nbt = pumpkin_nbt::compound::NbtCompound::new();
        match self {
            Self::OpenUrl { url } => {
                nbt.put_string("type", "minecraft:open_url".into());
                nbt.put_string("url", url.clone());
            }
            Self::Custom { id, payload } => {
                nbt.put_string("type", "minecraft:custom".into());
                nbt.put_string("id", id.clone());
                if let Some(payload) = payload {
                    let bytes: Box<[i8]> = payload.iter().map(|b| b.cast_signed()).collect();
                    nbt.put("payload", pumpkin_nbt::tag::NbtTag::ByteArray(bytes));
                }
            }
        }
        nbt
    }
}

#[derive(Clone, Debug)]
pub enum DialogBody {
    PlainMessage { contents: TextComponent },
    Item { item: i32 }, // TODO: ItemStack serialization to NBT
}

#[derive(Clone, Debug)]
pub enum DialogInput {
    Boolean {
        label: TextComponent,
        default_value: bool,
    },
    Text {
        label: TextComponent,
        placeholder: TextComponent,
        default_value: String,
    },
    NumberRange {
        label: TextComponent,
        min: f32,
        max: f32,
        initial: f32,
        step: f32,
        label_format: Option<String>,
    },
    SingleOption {
        label: TextComponent,
        options: Vec<TextComponent>,
        initial_index: u32,
    },
}

#[derive(Clone, Debug)]
pub struct ActionButton {
    pub text: TextComponent,
    pub tooltip: Option<TextComponent>,
    pub width: Option<u32>,
    pub action: DialogAction,
}

#[derive(Clone, Debug)]
pub enum DialogAction {
    OpenUrl {
        url: String,
    },
    Custom {
        id: String,
        payload: Option<Vec<u8>>,
    },
}

#[derive(Clone, Debug)]
pub struct DialogLink {
    pub label: crate::Label,
    pub url: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumpkin_data::packet::CURRENT_MC_VERSION;
    use pumpkin_nbt::tag::NbtTag;

    #[test]
    fn multi_action_dialog_uses_vanilla_field_names() {
        let dialog = Dialog {
            r#type: "minecraft:multi_action".into(),
            title: TextComponent::text("Menu"),
            body: vec![DialogBody::PlainMessage {
                contents: TextComponent::text("Pick"),
            }],
            inputs: vec![],
            buttons: vec![ActionButton {
                text: TextComponent::text("Go"),
                tooltip: None,
                width: Some(5000),
                action: DialogAction::Custom {
                    id: "demo:go/1".into(),
                    payload: None,
                },
            }],
            links: vec![],
            exit_action: None,
            after_action: Some("pop".into()),
            can_close_with_escape: true,
            external_title: None,
        };
        let nbt = dialog.to_nbt(&CURRENT_MC_VERSION);
        assert_eq!(nbt.get_string("type"), Some("minecraft:multi_action"));
        assert_eq!(nbt.get_string("after_action"), Some("close"));
        let Some(NbtTag::List(actions)) = nbt.get("actions") else {
            panic!("no actions")
        };
        let NbtTag::Compound(button) = &actions[0] else {
            panic!("bad button")
        };
        assert_eq!(button.get_int("width"), Some(1024));
        let action = button.get_compound("action").expect("action");
        assert_eq!(action.get_string("type"), Some("minecraft:custom"));
        assert_eq!(action.get_string("id"), Some("demo:go/1"));
        let Some(NbtTag::List(body)) = nbt.get("body") else {
            panic!("no body")
        };
        assert_eq!(body.len(), 1);
    }
}
