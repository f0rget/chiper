use std::fs::File;
use std::io::{self, stdin, Read, Write};
use std::thread;
use std::time::{self, SystemTime};

use crate::screen::Screen;

/*
 * Memory mapping, total 4k (0x1000)
 * ---------------------------------------------------------------
 * | 0x0-0x200 | 0x200 - 0xEA0 | 0xEA0 - 0xEFF |  0xF00 - 0xFFF  |
 * |interpreter| available mem |  call stack   | display refresh |
 * ---------------------------------------------------------------
 */

const MEMORY_START: usize = 0x200;
const MEMORY_SIZE: usize = 0x1000;
pub const SCREEN_WIDTH: u32 = 64;
pub const SCREEN_HEIGHT: u32 = 32;

const STACK_MEMORY_END: usize = 0xf00;
const SCREEN_MEMORY_START: u32 = 0xf00;
//const SCREEN_MEMORY_END: u32 = 0xfff;

// TODO: use logger?
#[cfg(debug_assertions)]
macro_rules! debug {
    ($( $args:expr ),*) => { print!( $( $args ),* ); }
}

#[cfg(not(debug_assertions))]
macro_rules! debug {
    ($( $args:expr ),*) => {};
}

#[derive(Debug)]
struct Opcode(u8, u8);

impl Opcode {
    fn high_nib(byte: u8) -> u8 {
        byte >> 4
    }

    fn low_nib(byte: u8) -> u8 {
        byte & 0x0f
    }

    // return as usize since it's used only as index for V[] registers
    fn x(&self) -> usize {
        Opcode::low_nib(self.0).into()
    }

    // return as usize since it's used only as index for V[] registers
    fn y(&self) -> usize {
        Opcode::high_nib(self.1).into()
    }

    fn n(&self) -> u8 {
        Opcode::low_nib(self.1)
    }

    fn nnn(&self) -> u16 {
        (Opcode::low_nib(self.0) as u16) << 8 | self.1 as u16
    }

    fn disassemble(&self, pc: usize) {
        debug!("{:04x}:\t{:02x} {:02x}\t", pc, self.0, self.1);
        match Opcode::high_nib(self.0) {
            0x00 => match self.1 {
                0xe0 => {
                    debug!("dclr");
                }
                0xee => {
                    debug!("ret");
                }
                _ => {
                    debug!("UNKNOWN");
                }
            },
            0x01 => {
                // Jumps to address NNN.
                debug!("jmp\t\t{:03x}", self.nnn());
            }
            0x02 => {
                debug!("call\t\t{:03x}", self.nnn());
            }
            0x03 => {
                // Skips the next instruction if VX equals NN.
                // Usually the next instruction is a jump to skip a code block
                debug!("skipifeq\t\tV{:01x}, {:02x}", self.x(), self.1);
            }
            0x04 => {
                // Skips the next instruction if VX doesn't equal NN. (Usually the next instruction
                // is a jump to skip a code block)
                debug!("skipifne\t\tV{:01x}, {:02x}", self.x(), self.1);
            }
            0x05 => {
                // Skips the next instruction if VX equals VY.
                // Usually the next instruction is a jump to skip a code block
                debug!("skipifeq\t\tV{:01x}, V{:01x}", self.x(), self.y());
            }
            0x06 => {
                // Sets VX to NN
                debug!("mov\t\tV{:01x}, {:02x}", self.x(), self.1);
            }
            0x07 => {
                // Adds NN to VX. (Carry flag is not changed)
                debug!("add\t\tV{:01x}, {:02x}", self.x(), self.1);
            }
            0x08 => {
                match Opcode::low_nib(self.1) {
                    0x0 => {
                        // Sets VX to the value of VY.
                        debug!("mov\t\tV{:01x}, V{:01x}", self.x(), self.y());
                    }
                    0x1 => {
                        // Sets VX to VX or VY. (Bitwise OR operation)
                        debug!("or\t\tV{:01x}, V{:01x}", self.x(), self.y());
                    }
                    0x2 => {
                        // Sets VX to VX and VY. (Bitwise AND operation)
                        debug!("and\t\tV{:01x}, V{:01x}", self.x(), self.y());
                    }
                    0x3 => {
                        // Sets VX to VX xor VY.
                        debug!("xor\t\tV{:01x}, V{:01x}", self.x(), self.y());
                    }
                    0x4 => {
                        // Adds VY to VX. VF is set to 1 when there's a carry, and to 0 when there isn't.
                        debug!("addwc\t\tV{:01x}, V{:01x}", self.x(), self.y());
                    }
                    0x5 => {
                        // VY is subtracted from VX. VF is set to 0 when there's a borrow, and 1 when there isn't.
                        debug!("subwc\t\tV{:01x}, V{:01x}", self.x(), self.y());
                    }
                    0x6 => {
                        // Stores the least significant bit of VX in VF and then shifts VX to the right by 1
                        debug!("shr\t\tV{:01x}", self.x());
                    }
                    0x7 => {
                        // Sets VX to VY minus VX. VF is set to 0 when there's a borrow, and 1 when there isn't.
                        debug!("subwc\t\tV{:01x}, V{:01x}, V{:01x}", self.x(), self.y(), self.x());
                    }
                    0xe => {
                        // Stores the most significant bit of VX in VF and then shifts VX to the left by 1
                        debug!("shl\t\tV{:01x}", self.x());
                    }
                    _ => debug!("UNKNOWN")
                }

            }
            0x0a => {
                //Sets I to the address NNN

                debug!("mov\t\tI, {:03x}", self.nnn());
            }
            0x0c => {
                debug!("rnd\t\tV{:01x}", self.x());
            }
            0x0d => {
                // draw(Vx,Vy,N)
                debug!(
                    "draw\t\tV{:01x}, V{:01x}, {:01x}",
                    self.x(),
                    self.y(),
                    self.n()
                );
            }
            0x0f => match self.1 {
                0x1e => {
                    // Adds VX to I. VF is not affected
                    debug!("add\t\tI, V{:01x}", self.x());
                }
                0x55 => {
                    // Stores V0 to VX (including VX) in memory starting at
                    // address I. The offset from I is increased by 1 for each
                    // value written, but I itself is left unmodified
                    debug!("movm\t\tI, V0-V{:01x}", self.x());
                }
                0x65 => {
                    // Fills V0 to VX (including VX) with values from memory
                    // starting at address I. The offset from I is increased by
                    // 1 for each value written, but I itself is left unmodified
                    debug!("movm\t\tV0-V{:01x}, I", self.x());
                }
                _ => {
                    debug!("Opcode is not handled yet");
                }
            },
            _ => {
                debug!("Opcode is not handled yet");
            }
        }
        debug!("\n");
    }
}

fn rand(seed: u64) -> u64 {
    // https://en.wikipedia.org/wiki/Xorshift
    let mut rnd = seed;
    rnd ^= rnd << 13;
    rnd ^= rnd >> 7;
    rnd ^= rnd << 17;
    return rnd;
}

pub struct Chip8<T> {
    ///  16 8-bit data registers named V0 to VF
    v: [u8; 16],
    /// Memory address register
    i: u16,
    /// Stack pointer
    sp: usize,
    /// Program counter
    pc: usize,
    /*
       uint8_t     delay;
       uint8_t     sound;
       uint8_t     *screen;  //this is memory[0xF00];
    */
    /// RAM
    memory: [u8; MEMORY_SIZE],
    /// amount of memory occupied by rom
    used_memory: usize,

    screen: T,

    /// Seed for a random number generator
    seed: u64,
}

impl<T: Screen> Chip8<T> {
    pub fn new(screen: T) -> Chip8<T> {
        Chip8 {
            v: [0; 16],
            i: 0,
            sp: STACK_MEMORY_END,
            pc: MEMORY_START,
            memory: [0; MEMORY_SIZE],
            used_memory: 0,
            screen: screen,
            seed: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("Time go backwards!")
                .as_secs(),
        }
    }

    pub fn load_rom(&mut self, rom_path: &str) -> io::Result<()> {
        let mut file = File::open(rom_path)?;
        let mut buffer = Vec::<u8>::new();

        // read the whole file into buffer
        file.read_to_end(&mut buffer)?;

        // CHIP-8 convention puts programs in memory at `MEMORY_START`
        // They will all have hardcoded addresses expecting that
        self.memory[MEMORY_START..MEMORY_START + buffer.len()].copy_from_slice(&buffer[..]);
        self.used_memory = buffer.len();
        Ok(())
    }

    /// Dump all Chip8 registers, but not memory
    pub fn dump_registers(&self) {
        print!("REGISTERS:\n");
        print!("V = [");
        for i in 0..16 {
            print!("{:01x}:{:02x} ", i, self.v[i]);
        }
        print!("]\n");

        print!("I  = {:02x?}\n", self.i);
        print!("SP = {:02x?}\n", self.sp);
        print!("PC = {:02x?}\n", self.pc);
    }

    pub fn dump_memory(&self) {
        let mut pc = MEMORY_START;
        let mut memory_end = MEMORY_START + self.used_memory;
        // ensure that we are 2 bytes alighned
        // XXX: not sure it's usefull for actual program or just for debugging
        if memory_end %2 != 0 {
            memory_end += 1;
        }
        for two_bytes in self.memory[MEMORY_START..memory_end].chunks(2) {
            let opcode = Opcode(two_bytes[0], two_bytes[1]);
            opcode.disassemble(pc);
            pc += 2;
        }
    }

    fn inc_pc(&mut self) {
        self.pc += 2;
    }

    fn emulate_op(&mut self) {
        let opcode = Opcode(self.memory[self.pc], self.memory[self.pc + 1]);
        opcode.disassemble(self.pc);

        match Opcode::high_nib(opcode.0) {
            0x00 => match opcode.1 {
                0xe0 => self.op_disp_clear(),
                0xee => {
                    //Returns from a subroutine.

                    // restore pc from the stack memory
                    self.pc = (((self.memory[self.sp] as u16) << 8) | (self.memory[self.sp + 1]) as u16) as usize;
                    // increase stack size back
                    self.sp += 2;
                }
                _ => unimplemented!(),
            },
            0x01 => {
                // Jumps to address NNN.
                let target = opcode.nnn();
                if target as usize == self.pc {
                    print!("Press ENTER to exit..\n");
                    let mut buffer = [0];
                    stdin().read_exact(&mut buffer).unwrap();
                    std::process::exit(0);
                }
                self.pc = target.into();
            }
            0x02 => {
                // Calls subroutine at NNN.

                // store current value of next instruction on the stack

                self.sp -= 2;
                self.memory[self.sp] = ((self.pc + 2) >> 8) as u8;
                self.memory[self.sp + 1] = ((self.pc + 2) & 0xff) as u8;

                self.pc = opcode.nnn().into();
            }
            0x03 => {
                // Skips the next instruction if VX equals NN.
                // Usually the next instruction is a jump to skip a code block
                if self.v[opcode.x()] == opcode.1 {
                    self.inc_pc();
                }
            }
            0x04 => {
                // Skips the next instruction if VX doesn't equal NN.
                // Usually the next instruction is a jump to skip a code block
                if self.v[opcode.x()] != opcode.1 {
                    self.inc_pc();
                }
            }
            0x05 => {
                // Skips the next instruction if VX equals VY
                // Usually the next instruction is a jump to skip a code block
                if self.v[opcode.x()] == self.v[opcode.y()] {
                    self.inc_pc();
                }
            }
            0x06 => {
                //Sets VX to NN
                self.v[opcode.x()] = opcode.1;
            }
            0x07 => {
                // Adds NN to VX. (Carry flag is not changed)
                self.v[opcode.x()] = self.v[opcode.x()].wrapping_add(opcode.1);
            }
            0x08 => {
                match Opcode::low_nib(opcode.1) {
                    0x0 => {
                        // Sets VX to the value of VY.
                        self.v[opcode.x()] = self.v[opcode.y()];
                    }
                    0x1 => {
                        // Sets VX to VX or VY. (Bitwise OR operation)
                        self.v[opcode.x()] |= self.v[opcode.y()];
                    }
                    0x2 => {
                        // Sets VX to VX and VY. (Bitwise AND operation)
                        self.v[opcode.x()] &= self.v[opcode.y()];
                    }
                    0x3 => {
                        // Sets VX to VX xor VY.
                        self.v[opcode.x()] ^= self.v[opcode.y()];
                    }
                    0x4 => {
                        // Adds VY to VX. VF is set to 1 when there's a carry, and to 0 when there isn't.
                        let (val, carry) = self.v[opcode.x()].overflowing_add(self.v[opcode.y()]);
                        self.v[opcode.x()] = val;
                        self.v[0xf] = carry as u8;
                    }
                    0x5 => {
                        // VY is subtracted from VX. VF is set to 0 when there's a borrow, and 1 when there isn't.
                        let (val, borrow) = self.v[opcode.x()].overflowing_sub(self.v[opcode.y()]);
                        self.v[opcode.x()] = val;
                        self.v[0xf] = (!borrow) as u8;
                    }
                    0x6 => {
                        // Stores the least significant bit of VX in VF and then shifts VX to the right by 1
                        self.v[0xf] = self.v[opcode.x()] & 0x1;
                        self.v[opcode.x()] >>= 1;
                    }
                    0x7 => {
                        // Sets VX to VY minus VX. VF is set to 0 when there's a borrow, and 1 when there isn't.
                        let (val, borrow) = self.v[opcode.y()].overflowing_sub(self.v[opcode.x()]);
                        self.v[opcode.x()] = val;
                        self.v[0xf] = (!borrow) as u8;
                    }
                    0xe => {
                        // Stores the most significant bit of VX in VF and then shifts VX to the left by 1
                        self.v[0xf] = self.v[opcode.x()] & 0x1;
                        self.v[opcode.x()] <<= 1;
                    }
                    _ => unreachable!("UNKNOW COMMAND: {:02x} {:02x}", opcode.0, opcode.1)
                }
            }
            0x0a => {
                //Sets I to the address NNN
                self.i = opcode.nnn();
            }
            0x0c => {
                // Sets VX to the result of a bitwise and operation on a
                // random number (0 to 255) and NN
                self.v[opcode.x()] = (self.rand_gen()) as u8 & opcode.1;
            }
            0x0d => {
                self.op_draw(
                    self.v[opcode.x()].into(),
                    self.v[opcode.y()].into(),
                    opcode.n(),
                );
            }
            0x0f => match opcode.1 {
                0x1e => {
                    // Adds VX to I. VF is not affected
                    self.i = self.i.wrapping_add(self.v[opcode.x()].into());
                }
                0x55 => {
                    // Stores V0 to VX (including VX) in memory starting at
                    // address I. The offset from I is increased by 1 for each
                    // value written, but I itself is left unmodified
                    for i in 0..opcode.x() {
                        self.memory[self.i as usize + i] = self.v[i];
                    }
                    self.i += opcode.x() as u16 + 1;
                }
                0x65 => {
                    // Fills V0 to VX (including VX) with values from memory
                    // starting at address I. The offset from I is increased by
                    // 1 for each value written, but I itself is left unmodified
                    for i in 0..opcode.x() {
                        self.v[i] = self.memory[self.i as usize + i];
                    }
                    self.i += opcode.x() as u16 + 1;
                }
                _ => unimplemented!(),
            },
            _ => unimplemented!(),
        }

        match Opcode::high_nib(opcode.0) {
            // one of the JUMP instruction, this will change the PC by itself,
            // not need to increment it
            0x01 => {}
            // regular opcode, move forward to the next one
            _ => self.inc_pc(),
        }
    }

    /// Clears the screen
    fn op_disp_clear(&mut self) {
        // TODO: think should we use sdl2 or webasm, or both
        // Ideally would be to provide trait:Display(Renderer) and anyone who implements
        // it can be passed to chip8 to be use as graphical interface
        self.screen.clear();
        self.screen.present();
    }

    /// Draw the sprite
    fn op_draw(&mut self, x: usize, y: usize, len: u8) {
        // Draws a sprite at coordinate (VX, VY) that has a width of 8 pixels and a height
        // of N+1 pixels. Each row of 8 pixels is read as bit-coded starting from memory
        // location I; I value doesn’t change after the execution of this instruction. As
        // described above, VF is set to 1 if any screen pixels are flipped from set to
        // unset when the sprite is drawn, and to 0 if that doesn’t happen
        let mut cy;
        for i in 0..len {
            cy = y + i as usize;
            if cy >= SCREEN_HEIGHT as usize {
                // sprite goes out of screen, stop drawing
                break;
            }

            let sprite_line = self.memory[(self.i + i as u16) as usize];
            let mut cx = x;
            for bi in (0..8).rev() {
                let mut px = ((sprite_line & (1 << bi)) != 0) as u8;

                if px != 0 {
                    if cx >= SCREEN_WIDTH as usize {
                        // sprite goes out of screen, stop drawing line
                        break;
                    }
                    // Determine the address of the effected byte on the screen
                    let screen_line_idx = SCREEN_MEMORY_START as usize + cy * 8 + cx / 8;
                    let screen_line = self.memory[screen_line_idx];
                    // Determine the effected bit in the byte
                    let screen_px = screen_line & (1 << (cx % 8));
                    if screen_px != 0 {
                        self.v[0xf] = 1;
                    }

                    // Write the effected bit to the screen memory
                    self.memory[screen_line_idx] ^= px;

                    // draw px
                    px ^= screen_px;
                    if px == 0 {
                        self.screen.clear_px(cx as i32, cy as i32);
                    } else {
                        self.screen.draw_px(cx as i32, cy as i32);
                    }
                }
                cx += 1;
            }
        }
        self.screen.present();
    }

    fn rand_gen(&mut self) -> u64 {
        let number = rand(self.seed);
        self.seed = number;
        return number;
    }

    pub fn emulate(&mut self) {
        loop {
            self.emulate_op();
            thread::sleep(time::Duration::from_millis(30));
        }
    }

    pub fn debugger(&mut self) -> io::Result<()> {
        print!("Enter debug mode:\n");
        print!("\t'r' - to run program\n");
        print!("\t'n' - for next instruction\n");
        print!("\t'q' - to exit\n");
        let mut buffer = String::new();
        let mut last_cmd = String::new();
        loop {
            print!("(chiper - db) ");
            io::stdout().flush().ok().expect("Could not flush stdout");
            stdin().read_line(&mut buffer)?;
            let mut cmd = buffer.trim_end();
            if cmd.is_empty() {
                cmd = &last_cmd;
            } else {
                last_cmd = cmd.to_string();
            }
            match cmd {
                "n" => {
                    self.emulate_op();
                    self.dump_registers();
                }
                "r" => loop {
                    self.emulate_op();
                    self.dump_registers();
                },
                "q" => {
                    break;
                }
                unknown => {
                    eprint!("Unknown debug command '{}'\n", unknown);
                }
            }
            buffer.clear();
        }
        Ok(())
    }
}
