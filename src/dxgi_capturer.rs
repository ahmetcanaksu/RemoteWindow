use windows::core::ComInterface;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::*;

pub struct DxgiCapturer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    desc: DXGI_OUTDUPL_DESC,
}

impl DxgiCapturer {
    pub fn new(name: String) -> Self {
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1().unwrap();

            // 1. Find the SPECIFIC adapter that owns this monitor
            let (adapter, output1) = Self::find_dxgi_output_for_name(&factory, &name)
                .expect("Could not find DXGI output matching GDI name");

            let mut device = None;
            let mut context = None;

            // 2. Create the device for THAT SPECIFIC adapter
            // Note: When passing an adapter, DriverType MUST be D3D_DRIVER_TYPE_UNKNOWN
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .unwrap();

            let (device, context) = (device.unwrap(), context.unwrap());
            println!("Device and Context created for adapter: {}", name);
            println!("Device : {:?}", device);
            println!("Context: {:?}", context);
            let duplication = output1.DuplicateOutput(&device).unwrap();
            let mut desc = DXGI_OUTDUPL_DESC::default();
            duplication.GetDesc(&mut desc);

            Self {
                device,
                context,
                duplication,
                desc,
            }
        }
    }

    pub fn capture_frame(&mut self) -> Option<&[u8]> {
        unsafe {
            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource = None;

            // Acquire the frame from the GPU
            if self
                .duplication
                .AcquireNextFrame(100, &mut frame_info, &mut resource)
                .is_err()
            {
                return None;
            }

            let texture: ID3D11Texture2D = resource.unwrap().cast().unwrap();
            let mut tex_desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut tex_desc);

            // Create a "Staging Texture" to move data from GPU to CPU
            tex_desc.Usage = D3D11_USAGE_STAGING;
            tex_desc.BindFlags = 0;
            tex_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            tex_desc.MiscFlags = 0;

            let mut staging_texture = None;
            self.device
                .CreateTexture2D(&tex_desc, None, Some(&mut staging_texture))
                .unwrap();
            let staging_texture = staging_texture.unwrap();

            // Copy GPU -> CPU
            self.context.CopyResource(&staging_texture, &texture);

            // Map the memory so we can read it
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&staging_texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .unwrap();

            println!(
                "Frame captured: {}x{}, Pitch: {}, Size: {} bytes",
                tex_desc.Width,
                tex_desc.Height,
                mapped.RowPitch,
                mapped.RowPitch * tex_desc.Height
            );
            let view: &[u32] = std::slice::from_raw_parts(
                mapped.pData as *const u32,
                (tex_desc.Width * tex_desc.Height) as usize,
            );

            let cxe: &[u8] = bytemuck::cast_slice(view);
            //let result = view.to_vec();

            self.context.Unmap(&staging_texture, 0);
            self.duplication.ReleaseFrame().unwrap();

            Some(cxe)
        }
    }

    pub fn find_dxgi_output_for_name(
        factory: &IDXGIFactory1,
        target_name: &str,
    ) -> Option<(IDXGIAdapter1, IDXGIOutput1)> {
        unsafe {
            let mut adapter_index = 0;

            while let Ok(adapter) = factory.EnumAdapters1(adapter_index) {
                let mut output_index = 0;
                while let Ok(output) = adapter.EnumOutputs(output_index) {
                    let mut desc = DXGI_OUTPUT_DESC::default();
                    output.GetDesc(&mut desc).unwrap();

                    // Convert DXGI DeviceName (UTF16) to String
                    let dxgi_name = String::from_utf16_lossy(&desc.DeviceName)
                        .trim_matches(char::from(0))
                        .to_string();

                    if dxgi_name == target_name {
                        return Some((adapter, output.cast().unwrap()));
                    }
                    output_index += 1;
                }
                adapter_index += 1;
            }
        }
        None
    }

    // Pass a mutable buffer in so we don't have dangling references!
    pub fn capture_frame_into(&mut self, target_buffer: &mut [u8]) -> bool {
        unsafe {
            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource = None;

            // 1. Grab the frame from the GPU
            if self
                .duplication
                .AcquireNextFrame(100, &mut frame_info, &mut resource)
                .is_err()
            {
                return false;
            }

            let texture: ID3D11Texture2D = resource.unwrap().cast().unwrap();
            let mut tex_desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut tex_desc);

            let width_bytes = (tex_desc.Width * 4) as usize;

            println!(
                "[CRITICAL] Texture is {}x{}, but buffer only fits 1080p!",
                tex_desc.Width, tex_desc.Height
            );

            // 2. Create a Staging Texture that matches the monitor perfectly
            // We mutate a local copy of tex_desc to ensure CPU access
            let mut staging_desc = tex_desc;
            staging_desc.Usage = D3D11_USAGE_STAGING;
            staging_desc.BindFlags = 0;
            staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            staging_desc.MiscFlags = 0;
            staging_desc.MipLevels = 1;
            staging_desc.ArraySize = 1;

            // AMD FIX: Staging textures MUST be single-sampled (Count = 1)
            staging_desc.SampleDesc.Count = 1;
            staging_desc.SampleDesc.Quality = 0;

            let mut staging_texture = None;
            self.device
                .CreateTexture2D(&staging_desc, None, Some(&mut staging_texture))
                .unwrap();
            let staging_texture = staging_texture.unwrap();

            // 3. Move pixels GPU -> CPU Staging
            // ResolveSubresource handles cases where the monitor uses MSAA or odd formats
            if tex_desc.SampleDesc.Count > 1 {
                self.context
                    .ResolveSubresource(&staging_texture, 0, &texture, 0, tex_desc.Format);
            } else {
                self.context.CopyResource(&staging_texture, &texture);
            }

            // 4. Map the memory and copy to your Rust buffer
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            if self
                .context
                .Map(&staging_texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .is_ok()
            {
                let src_ptr = mapped.pData as *const u8;
                let pitch = mapped.RowPitch as usize;
                let width_bytes = tex_desc.Width as usize * 4;

                // Row-by-row copy to fix the "Purple Tear" alignment
                for row in 0..tex_desc.Height as usize {
                    let src_row = src_ptr.add(row * pitch);
                    let dest_row = target_buffer.as_mut_ptr().add(row * width_bytes);
                    std::ptr::copy_nonoverlapping(src_row, dest_row, width_bytes);
                }

                self.context.Unmap(&staging_texture, 0);
            }

            // 5. Release the frame so the GPU can draw the next one
            self.duplication.ReleaseFrame().unwrap();
            true
        }
    }
}
