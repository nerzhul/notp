use crate::qr_import::{parse_otpauth, OtpParams};
use anyhow::{anyhow, bail, Context, Result};
use std::ffi::CString;
use std::mem::size_of;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::RawFd;
use std::ptr;
use std::slice;
use std::thread::{sleep, JoinHandle};
use std::time::Duration;

const FRAME_WIDTH: u32 = 640;
const FRAME_HEIGHT: u32 = 480;
const V4L2_BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
const V4L2_MEMORY_MMAP: u32 = 1;
const V4L2_PIX_FMT_YUYV: u32 = 0x5659_5559;
const V4L2_PIX_FMT_YVYU: u32 = 0x5559_5659;
const V4L2_PIX_FMT_UYVY: u32 = 0x5956_5559;
const V4L2_PIX_FMT_VYUY: u32 = 0x5955_5956;
const V4L2_PIX_FMT_MJPEG: u32 = 0x4750_4a4d;
const V4L2_PIX_FMT_RGB24: u32 = 0x3342_4752;
const V4L2_PIX_FMT_BGR24: u32 = 0x3352_4742;
const V4L2_PIX_FMT_GREY: u32 = 0x5945_5247;

const TYP_V: u8 = b'V';
const DIR_WRITE: u32 = 1;
const DIR_RW: u32 = 3;
const DIR_READ: u32 = 2;
const V4L2_CAP_VIDEO_CAPTURE: u32 = 0x0000_0001;

const NR_S_FMT: u32 = 5;
const NR_G_FMT: u32 = 4;
const NR_ENUM_FMT: u32 = 2;
const NR_QUERYCAP: u32 = 0;
const NR_REQBUFS: u32 = 8;
const NR_QUERYBUF: u32 = 9;
const NR_QBUF: u32 = 15;
const NR_DQBUF: u32 = 17;
const NR_STREAMON: u32 = 18;
const NR_STREAMOFF: u32 = 19;

const fn ioc(dir: u32, typ: u8, nr: u32, size: usize) -> libc::c_ulong {
    ((dir as libc::c_ulong) << 30)
        | ((typ as libc::c_ulong) << 8)
        | (nr as libc::c_ulong)
        | ((size as libc::c_ulong) << 16)
}

#[repr(C)]
#[derive(Copy, Clone)]
struct v4l2_pix_format {
    width: u32,
    height: u32,
    pixelformat: u32,
    field: u32,
    bytesperline: u32,
    sizeimage: u32,
    colorspace: u32,
    priv_: u32,
    flags: u32,
    ycbcr_enc: u32,
    quantization: u32,
    xfer_func: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
union v4l2_format_data {
    pix: v4l2_pix_format,
    raw: [u8; 200],
}

#[repr(C)]
struct v4l2_format {
    type_: u32,
    fmt: v4l2_format_data,
}

#[repr(C)]
struct v4l2_capability {
    driver: [u8; 16],
    card: [u8; 32],
    bus_info: [u8; 32],
    version: u32,
    capabilities: u32,
    device_caps: u32,
    reserved: [u32; 3],
}

#[repr(C)]
struct v4l2_requestbuffers {
    count: u32,
    type_: u32,
    memory: u32,
    reserved: [u32; 2],
}

#[repr(C)]
struct v4l2_fmtdesc {
    index: u32,
    type_: u32,
    flags: u32,
    reserved: [u32; 4],
    description: [u8; 32],
    pixelformat: u32,
    reserved2: [u32; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct v4l2_timecode {
    type_: u32,
    flags: u32,
    frames: u32,
    seconds: u8,
    minutes: u8,
    hours: u8,
    padding: [u8; 9],
}

#[repr(C)]
union v4l2_buffer_m {
    offset: u32,
    userptr: u32,
    planes: *mut libc::c_void,
}

#[repr(C)]
struct v4l2_buffer {
    index: u32,
    type_: u32,
    bytesused: u32,
    flags: u32,
    field: u32,
    timestamp: libc::timeval,
    timecode: v4l2_timecode,
    sequence: u32,
    memory: u32,
    m: v4l2_buffer_m,
    length: u32,
    input: u32,
    reserved: u32,
}

pub fn start_scan<F>(on_result: F) -> Result<JoinHandle<()>>
where
    F: FnOnce(Result<OtpParams>) + Send + 'static,
{
    let handle = std::thread::spawn(move || {
        let result = scan_loop();
        on_result(result);
    });
    Ok(handle)
}

fn find_camera() -> Option<std::ffi::OsString> {
    for index in 0..16 {
        let path = format!("/dev/video{}", index);
        let cpath = match CString::new(path.as_bytes()) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let fd = unsafe { libc::open(cpath.as_ptr(), libc::O_RDWR) };
        if fd < 0 {
            continue;
        }
        if is_capture_device(fd) {
            unsafe {
                libc::close(fd);
            }
            return Some(std::ffi::OsString::from(path));
        }
        unsafe {
            libc::close(fd);
        }
    }
    None
}

fn is_capture_device(fd: RawFd) -> bool {
    let mut caps = v4l2_capability {
        driver: [0; 16],
        card: [0; 32],
        bus_info: [0; 32],
        version: 0,
        capabilities: 0,
        device_caps: 0,
        reserved: [0; 3],
    };
    if call_ioctl(fd, DIR_READ, NR_QUERYCAP, &mut caps).is_err() {
        return false;
    }
    let caps_field = if caps.capabilities != 0 {
        caps.capabilities
    } else {
        caps.device_caps
    };
    caps_field & V4L2_CAP_VIDEO_CAPTURE != 0
}

fn scan_loop() -> Result<OtpParams> {
    let device_path = match std::env::var_os("NOTP_VIDEO_DEVICE") {
        Some(path) if !path.is_empty() => path,
        _ => find_camera().ok_or_else(|| {
            anyhow!(
                "No /dev/video* device was found. Set the NOTP_VIDEO_DEVICE environment variable \
                 to point to your camera."
            )
        })?,
    };

    let path_c = CString::new(device_path.as_bytes())
        .context("The camera device path contains a null byte")?;
    let fd = unsafe { libc::open(path_c.as_ptr(), libc::O_RDWR) };
    if fd < 0 {
        let os_error = std::io::Error::last_os_error();
        return Err(anyhow!(
            "Unable to open the camera ({}): {} (the user might lack access to the video group).",
            path_c.to_string_lossy(),
            os_error
        ));
    }

    let mut mappings: Vec<(*mut u8, usize)> = Vec::new();

    let result = (|| -> Result<OtpParams> {
        let pixelformat = set_format(fd)?;
        let mut req = v4l2_requestbuffers {
            count: 4,
            type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
            memory: V4L2_MEMORY_MMAP,
            reserved: [0; 2],
        };
        call_ioctl(fd, DIR_RW, NR_REQBUFS, &mut req).context("VIDIOC_REQBUFS failed")?;

        for index in 0..req.count {
            let mut buf = zero_buffer(index);
            call_ioctl(fd, DIR_RW, NR_QUERYBUF, &mut buf)
                .with_context(|| format!("VIDIOC_QUERYBUF failed for buffer {index}"))?;
            let offset = unsafe { buf.m.offset };
            let ptr = unsafe {
                libc::mmap(
                    ptr::null_mut(),
                    buf.length as usize,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    fd,
                    offset as libc::off_t,
                )
            };
            if ptr == libc::MAP_FAILED {
                return Err(anyhow!(
                    "Unable to mmap the camera buffer: {}",
                    std::io::Error::last_os_error()
                ));
            }
            mappings.push((ptr as *mut u8, buf.length as usize));
        }

        for index in 0..req.count {
            let mut buf = zero_buffer(index);
            call_ioctl(fd, DIR_RW, NR_QBUF, &mut buf)
                .with_context(|| format!("VIDIOC_QBUF failed for buffer {index}"))?;
        }

        let mut stream_type = V4L2_BUF_TYPE_VIDEO_CAPTURE;
        call_ioctl(fd, DIR_WRITE, NR_STREAMON, &mut stream_type)
            .context("VIDIOC_STREAMON failed")?;

        let mut found: Option<OtpParams> = None;
        for _ in 0..1024 {
            let mut buf = v4l2_buffer {
                index: 0,
                type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
                memory: V4L2_MEMORY_MMAP,
                ..zero_buffer(0)
            };
            if let Err(error) = call_ioctl(fd, DIR_RW, NR_DQBUF, &mut buf) {
                return Err(anyhow!("VIDIOC_DQBUF failed: {}", error));
            }
            if let Some((ptr, length)) = mappings.get(buf.index as usize).copied() {
                let bytes = unsafe {
                    slice::from_raw_parts(ptr, buf.bytesused.min(length as u32) as usize)
                };
                let gray = match pixelformat {
                    V4L2_PIX_FMT_MJPEG => match mjpeg_to_gray(bytes) {
                        Ok(gray) => gray,
                        Err(_) => continue,
                    },
                    V4L2_PIX_FMT_RGB24 => rgb_to_gray(bytes, FRAME_WIDTH, FRAME_HEIGHT, false),
                    V4L2_PIX_FMT_BGR24 => rgb_to_gray(bytes, FRAME_WIDTH, FRAME_HEIGHT, true),
                    V4L2_PIX_FMT_GREY => grey_to_gray(bytes, FRAME_WIDTH, FRAME_HEIGHT),
                    _ => yuyv_to_gray(bytes, FRAME_WIDTH, FRAME_HEIGHT, pixelformat),
                };
                let mut prepared = rqrr::PreparedImage::prepare(gray.clone());
                let grids = prepared.detect_grids();
                if let Some(grid) = grids.into_iter().next() {
                    if let Ok((_meta, payload)) = grid.decode() {
                        if let Ok(params) = parse_otpauth(&payload) {
                            found = Some(params);
                        }
                    }
                }
            }
            let mut requeue = buf;
            call_ioctl(fd, DIR_RW, NR_QBUF, &mut requeue).ok();
            if found.is_some() {
                break;
            }
            sleep(Duration::from_millis(20));
        }

        let mut stream_type = V4L2_BUF_TYPE_VIDEO_CAPTURE;
        call_ioctl(fd, DIR_WRITE, NR_STREAMOFF, &mut stream_type).ok();

        found.ok_or_else(|| anyhow!("No QR code detected before timeout"))
    })();

    for (ptr, length) in mappings.drain(..) {
        unsafe {
            libc::munmap(ptr as *mut libc::c_void, length);
        }
    }
    unsafe {
        libc::close(fd);
    }

    result
}

fn set_format(fd: RawFd) -> Result<u32> {
    let candidates = [
        V4L2_PIX_FMT_YUYV,
        V4L2_PIX_FMT_UYVY,
        V4L2_PIX_FMT_YVYU,
        V4L2_PIX_FMT_VYUY,
        V4L2_PIX_FMT_RGB24,
        V4L2_PIX_FMT_BGR24,
        V4L2_PIX_FMT_GREY,
        V4L2_PIX_FMT_MJPEG,
    ];
    for pixelformat in candidates {
        let mut fmt = v4l2_format {
            type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
            fmt: v4l2_format_data {
                pix: v4l2_pix_format {
                    width: FRAME_WIDTH,
                    height: FRAME_HEIGHT,
                    pixelformat,
                    field: 0,
                    bytesperline: 0,
                    sizeimage: 0,
                    colorspace: 0,
                    priv_: 0,
                    flags: 0,
                    ycbcr_enc: 0,
                    quantization: 0,
                    xfer_func: 0,
                },
            },
        };
        if call_ioctl(fd, DIR_RW, NR_S_FMT, &mut fmt).is_ok() {
            let mut current = v4l2_format {
                type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
                fmt: v4l2_format_data {
                    pix: v4l2_pix_format {
                        width: 0,
                        height: 0,
                        pixelformat: 0,
                        field: 0,
                        bytesperline: 0,
                        sizeimage: 0,
                        colorspace: 0,
                        priv_: 0,
                        flags: 0,
                        ycbcr_enc: 0,
                        quantization: 0,
                        xfer_func: 0,
                    },
                },
            };
            call_ioctl(fd, DIR_RW, NR_G_FMT, &mut current).ok();
            let actual = unsafe { current.fmt.pix.pixelformat };
            if actual != 0 {
                return Ok(actual);
            }
        }
    }
    let mut current = v4l2_format {
        type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
        fmt: v4l2_format_data {
            pix: v4l2_pix_format {
                width: 0,
                height: 0,
                pixelformat: 0,
                field: 0,
                bytesperline: 0,
                sizeimage: 0,
                colorspace: 0,
                priv_: 0,
                flags: 0,
                ycbcr_enc: 0,
                quantization: 0,
                xfer_func: 0,
            },
        },
    };
    call_ioctl(fd, DIR_RW, NR_G_FMT, &mut current).ok();
    for pixelformat in enum_pixel_formats(fd) {
        let mut fmt = v4l2_format {
            type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
            fmt: v4l2_format_data {
                pix: v4l2_pix_format {
                    width: FRAME_WIDTH,
                    height: FRAME_HEIGHT,
                    pixelformat,
                    field: 0,
                    bytesperline: 0,
                    sizeimage: 0,
                    colorspace: 0,
                    priv_: 0,
                    flags: 0,
                    ycbcr_enc: 0,
                    quantization: 0,
                    xfer_func: 0,
                },
            },
        };
        if call_ioctl(fd, DIR_RW, NR_S_FMT, &mut fmt).is_ok() {
            let mut probe = v4l2_format {
                type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
                fmt: v4l2_format_data {
                    pix: v4l2_pix_format {
                        width: 0,
                        height: 0,
                        pixelformat: 0,
                        field: 0,
                        bytesperline: 0,
                        sizeimage: 0,
                        colorspace: 0,
                        priv_: 0,
                        flags: 0,
                        ycbcr_enc: 0,
                        quantization: 0,
                        xfer_func: 0,
                    },
                },
            };
            call_ioctl(fd, DIR_RW, NR_G_FMT, &mut probe).ok();
            let actual = unsafe { probe.fmt.pix.pixelformat };
            if actual != 0 {
                return Ok(actual);
            }
        }
    }
    bail!(
        "The camera accepted none of the requested pixel formats at {}x{} \
         (current pixel format is {:#x})",
        FRAME_WIDTH,
        FRAME_HEIGHT,
        unsafe { current.fmt.pix.pixelformat }
    );
}

fn enum_pixel_formats(fd: RawFd) -> Vec<u32> {
    let mut formats = Vec::new();
    for index in 0..64 {
        let mut desc = v4l2_fmtdesc {
            index,
            type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
            flags: 0,
            reserved: [0; 4],
            description: [0; 32],
            pixelformat: 0,
            reserved2: [0; 4],
        };
        if call_ioctl(fd, DIR_RW, NR_ENUM_FMT, &mut desc).is_err() {
            break;
        }
        if desc.pixelformat != 0 {
            formats.push(desc.pixelformat);
        }
    }
    formats
}

fn call_ioctl<T>(fd: RawFd, dir: u32, nr: u32, value: &mut T) -> Result<()> {
    let request = ioc(dir, TYP_V, nr, size_of::<T>());
    let result = unsafe { libc::ioctl(fd, request, value as *mut T) };
    if result < 0 {
        Err(anyhow!(
            "ioctl({}) failed: {}",
            nr,
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(())
    }
}

fn mjpeg_to_gray(bytes: &[u8]) -> Result<image::GrayImage> {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg)
        .context("Unable to decode the MJPEG frame")?;
    Ok(image.to_luma8())
}

fn rgb_to_gray(data: &[u8], width: u32, height: u32, bgr: bool) -> image::GrayImage {
    use image::{GrayImage, Luma};
    let mut image = GrayImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 3) as usize;
            if idx + 2 >= data.len() {
                return image;
            }
            let r = if bgr { data[idx + 2] } else { data[idx] };
            let g = data[idx + 1];
            let b = if bgr { data[idx] } else { data[idx + 2] };
            let gray = ((u32::from(r) * 299) + (u32::from(g) * 587) + (u32::from(b) * 114)) / 1000;
            image.put_pixel(x, y, Luma([gray as u8]));
        }
    }
    image
}

fn grey_to_gray(data: &[u8], width: u32, height: u32) -> image::GrayImage {
    use image::GrayImage;
    GrayImage::from_raw(width, height, data.to_vec())
        .unwrap_or_else(|| GrayImage::new(width, height))
}

fn zero_buffer(index: u32) -> v4l2_buffer {
    v4l2_buffer {
        index,
        type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
        bytesused: 0,
        flags: 0,
        field: 0,
        timestamp: libc::timeval {
            tv_sec: 0,
            tv_usec: 0,
        },
        timecode: v4l2_timecode {
            type_: 0,
            flags: 0,
            frames: 0,
            seconds: 0,
            minutes: 0,
            hours: 0,
            padding: [0; 9],
        },
        sequence: 0,
        memory: V4L2_MEMORY_MMAP,
        m: v4l2_buffer_m { offset: 0 },
        length: 0,
        input: 0,
        reserved: 0,
    }
}

fn yuyv_to_gray(data: &[u8], width: u32, height: u32, fourcc: u32) -> image::GrayImage {
    use image::{GrayImage, Luma};
    let mut image = GrayImage::new(width, height);
    let pairs_per_row = width / 2;
    let bytes_per_pair = match fourcc {
        V4L2_PIX_FMT_YUYV | V4L2_PIX_FMT_YVYU | V4L2_PIX_FMT_UYVY | V4L2_PIX_FMT_VYUY => 4,
        _ => return image,
    };
    let (y0_offset, y1_offset) = match fourcc {
        V4L2_PIX_FMT_YUYV => (0, 2),
        V4L2_PIX_FMT_YVYU => (2, 0),
        V4L2_PIX_FMT_UYVY => (1, 3),
        V4L2_PIX_FMT_VYUY => (3, 1),
        _ => return image,
    };
    for row in 0..height {
        for pair in 0..pairs_per_row {
            let offset = ((row * width * bytes_per_pair) + pair * bytes_per_pair) as usize;
            if offset + 3 >= data.len() {
                break;
            }
            let y0 = data[offset + y0_offset];
            let y1 = data[offset + y1_offset];
            image.put_pixel(pair * 2, row, Luma([y0]));
            if pair * 2 + 1 < width {
                image.put_pixel(pair * 2 + 1, row, Luma([y1]));
            }
        }
    }
    image
}
