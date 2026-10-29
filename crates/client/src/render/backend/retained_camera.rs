//! Temporary projection through the last completed scene's camera.
use crate::client::client::Client;

pub(crate) struct OverlayCameraScope<'a> {
    core: &'a mut Client,
    previous: Option<[i32; 5]>,
}

impl<'a> OverlayCameraScope<'a> {
    pub(crate) fn enter(core: &'a mut Client, camera: Option<[i32; 5]>) -> Self {
        let previous = camera.map(|camera| {
            let previous = [
                core.cam_x,
                core.cam_y,
                core.cam_z,
                core.cam_pitch,
                core.cam_yaw,
            ];
            [
                core.cam_x,
                core.cam_y,
                core.cam_z,
                core.cam_pitch,
                core.cam_yaw,
            ] = camera;
            previous
        });
        Self { core, previous }
    }

    pub(crate) fn core(&mut self) -> &mut Client {
        self.core
    }
}

impl Drop for OverlayCameraScope<'_> {
    fn drop(&mut self) {
        if let Some(camera) = self.previous {
            [
                self.core.cam_x,
                self.core.cam_y,
                self.core.cam_z,
                self.core.cam_pitch,
                self.core.cam_yaw,
            ] = camera;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ClientConfig;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    fn client() -> Client {
        Client::new(ClientConfig {
            host: "127.0.0.1".into(),
            port: 43594,
            cache_dir: format!("retained-camera-no-cache-{}", std::process::id()),
            members: true,
            lowmem: false,
        })
    }

    fn camera(core: &Client) -> [i32; 5] {
        [
            core.cam_x,
            core.cam_y,
            core.cam_z,
            core.cam_pitch,
            core.cam_yaw,
        ]
    }

    #[test]
    fn retained_overlay_camera_restores_on_unwind() {
        let mut core = client();
        let previous = camera(&core);
        assert!(catch_unwind(AssertUnwindSafe(|| {
            let mut scope = OverlayCameraScope::enter(&mut core, Some([11, 22, 33, 44, 55]));
            assert_eq!(camera(scope.core()), [11, 22, 33, 44, 55]);
            panic!("intentional overlay unwind");
        }))
        .is_err());
        assert_eq!(camera(&core), previous);
    }

    #[test]
    fn unused_camera_scope_does_not_restore_standalone_mutations() {
        let mut core = client();
        {
            let mut scope = OverlayCameraScope::enter(&mut core, None);
            scope.core().cam_x = 123;
        }
        assert_eq!(core.cam_x, 123);
    }
}
