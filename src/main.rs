use std::{fs::File, os::unix::io::AsFd};

use wayland_client::{
    delegate_noop,
    protocol::{
        wl_buffer, wl_compositor, wl_keyboard, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
    },
    Connection, Dispatch, QueueHandle, WEnum,
};

use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

fn main() {

    let args = std::env::args().collect::<Vec<String>>();
    if args.len() == 1 {
        panic!("no path for image specified\nUsage: quadtree [path]");
    }
    
    
    let image = match image::open(args[1].clone()) {
        Ok(image) => image,
        Err(err) => panic!("{}", err),
    };
    let (width, height) = (image.width(), image.height());
    
    let input = image.to_rgba8().to_vec();
    let mut output = vec![0_u8; input.len()];

    quadtree(
        input.as_ref(), 
        &mut output, 
        0, 0, 
        width as usize - 1, height as usize - 1, 
        width as usize, // the buffer width
        280,
        1
    );
    
    // wayland stuff
    let conn = Connection::connect_to_env().unwrap();

    let mut event_queue = conn.new_event_queue();
    let qhandle = event_queue.handle();

    let display = conn.display();
    display.get_registry(&qhandle, ());
    

    let mut frame_file = tempfile::tempfile().unwrap();
    std::io::Write::write_all(&mut frame_file, &output).unwrap();
    
    let mut state = State::new(frame_file, width, height);

    println!("Starting the example window app, press <ESC> to quit.");

    while state.running {
        event_queue.blocking_dispatch(&mut state).unwrap();
    }
}

fn quadtree(
    input: &[u8], 
    output: &mut [u8],
    x1: usize, 
    y1: usize, 
    x2: usize, 
    y2: usize,
    buffer_width: usize,
    color_threshold: u32,
    rect_min_size: usize,
) {
    // -- calculating the average color
    let mut r = 0_u32;
    let mut g = 0_u32;
    let mut b = 0_u32;
    let mut count = 0_u32;

    for y in y1..y2 {
        for x in x1 .. x2 {
            let index = y * buffer_width * 4 + x * 4;
            r += input[index] as u32;
            g += input[index+1] as u32;
            b += input[index+2] as u32;
            count += 1;
        }
    }
    r = r.checked_div(count).unwrap_or_default();
    g = g.checked_div(count).unwrap_or_default();
    b = b.checked_div(count).unwrap_or_default();
    // --


    let mut set_color = |index: usize, r: u8, g: u8, b: u8, a: u8| {
        output[index+3] = a;
        output[index+2] = r;
        output[index+1] = g;
        output[index] = b;
    };

    if r+g+b > color_threshold {
        for x in x1 .. x2 {
            let index = y1 * buffer_width * 4 + x * 4;
            set_color(index, r as u8, g as u8, b as u8, 255);

            let index = y2 * buffer_width * 4 + x * 4;
            set_color(index, r as u8, g as u8, b as u8, 255);
        }
        for y in y1 .. y2 {
            let index = y * buffer_width * 4 + x1 * 4;
            set_color(index, r as u8, g as u8, b as u8, 255);

            let index = y * buffer_width * 4 + x2 * 4;
            set_color(index, r as u8, g as u8, b as u8, 255);
        }
    }
    
    if x1.abs_diff(x2) > rect_min_size && y1.abs_diff(y2) > rect_min_size  {
        
        let mid_x = (x1 + x2) / 2;
        let mid_y = (y1 + y2) / 2;
        
        quadtree(input, output, x1, y1, mid_x, mid_y, buffer_width, color_threshold, rect_min_size);
        quadtree(input, output, mid_x, y1, x2, mid_y, buffer_width, color_threshold, rect_min_size);
        quadtree(input, output, x1, mid_y, mid_x, y2, buffer_width, color_threshold, rect_min_size);
        quadtree(input, output, mid_x, mid_y, x2, y2, buffer_width, color_threshold, rect_min_size);
    }
}

struct State {
    running: bool,
    base_surface: Option<wl_surface::WlSurface>,
    buffer: Option<wl_buffer::WlBuffer>,
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    xdg_surface: Option<(xdg_surface::XdgSurface, xdg_toplevel::XdgToplevel)>,
    configured: bool,
    width: u32,
    height: u32,
    frame_file: File,
}

impl State {
    fn new(file: File, width: u32, height: u32) -> Self {
        Self {
            running: true,
            base_surface: None,
            buffer: None,
            wm_base: None,
            xdg_surface: None,
            configured: false,

            width,
            height,
            frame_file: file,
        }
    }

    fn init_xdg_surface(&mut self, qh: &QueueHandle<State>) {
        let wm_base = self.wm_base.as_ref().unwrap();
        let base_surface = self.base_surface.as_ref().unwrap();

        let xdg_surface = wm_base.get_xdg_surface(base_surface, qh, ());
        let toplevel = xdg_surface.get_toplevel(qh, ());
        toplevel.set_title("A fantastic window!".into());

        base_surface.commit();

        self.xdg_surface = Some((xdg_surface, toplevel));
    }
}


impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            match &interface[..] {
                "wl_compositor" => {
                    let compositor =
                        registry.bind::<wl_compositor::WlCompositor, _, _>(name, version, qh, ());
                    let surface = compositor.create_surface(qh, ());
                    state.base_surface = Some(surface);

                    if state.wm_base.is_some() && state.xdg_surface.is_none() {
                        state.init_xdg_surface(qh);
                    }
                }
                "wl_shm" => {
                    let shm = registry.bind::<wl_shm::WlShm, _, _>(name, version, qh, ());

                    let pool = 
                        shm.create_pool(state.frame_file.as_fd(), (state.width * state.height * 4) as i32, qh, ());

                    let buffer = pool.create_buffer(
                        0,
                        state.width as i32,
                        state.height as i32,
                        (state.width* 4) as i32,
                        wl_shm::Format::Xrgb8888,
                        qh,
                        (),
                    );
                    state.buffer = Some(buffer.clone());
                }
                "wl_seat" => {
                    registry.bind::<wl_seat::WlSeat, _, _>(name, 1, qh, ());
                }
                "xdg_wm_base" => {
                    let wm_base = registry.bind::<xdg_wm_base::XdgWmBase, _, _>(name, 1, qh, ());
                    state.wm_base = Some(wm_base);

                    if state.base_surface.is_some() && state.xdg_surface.is_none() {
                        state.init_xdg_surface(qh);
                    }
                }
                _ => {}
            }
        }
    }
}

// Ignore events from these object types in this example.
delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_surface::WlSurface);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore wl_buffer::WlBuffer);



impl Dispatch<xdg_wm_base::XdgWmBase, ()> for State {
    fn event(
        _: &mut Self,
        wm_base: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for State {
    fn event(
        state: &mut Self,
        xdg_surface: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial, .. } = event {
            xdg_surface.ack_configure(serial);
            state.configured = true;
            let surface = state.base_surface.as_ref().unwrap();
            if let Some(ref buffer) = state.buffer {
                surface.attach(Some(buffer), 0, 0);
                surface.commit();
            }
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for State {
    fn event(
        state: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_toplevel::Event::Close {} = event {
            state.running = false;
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(capabilities) } = event {
            if capabilities.contains(wl_seat::Capability::Keyboard) {
                seat.get_keyboard(qh, ());
            }
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
          
        if let wl_keyboard::Event::Key { key, .. } = event {
            if key == 1 {
                // ESC key
                state.running = false;
            }
        }
    }
}
