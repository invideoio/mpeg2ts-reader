#[macro_use]
extern crate mpeg2ts_reader;
extern crate hex_slice;

use hex_slice::AsHex;
use mpeg2ts_reader::demultiplex;
use mpeg2ts_reader::packet;
use mpeg2ts_reader::parser;

use mpeg2ts_reader::pes;
use mpeg2ts_reader::psi;
use mpeg2ts_reader::StreamType;
use std::cmp;
use std::env;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::io::{BufReader, BufWriter};
use byteorder::{WriteBytesExt, LittleEndian};
use mpeg2ts_reader::parser::{HeaderCode, NALParser};
use mpeg2ts_reader::pes::Timestamp;

// This macro invocation creates an enum called DumpFilterSwitch, encapsulating all possible ways
// that this application may handle transport stream packets.  Each enum variant is just a wrapper
// around an implementation of the PacketFilter trait
packet_filter_switch! {
    DumpFilterSwitch<DumpDemuxContext> {
        // the DumpFilterSwitch::Pes variant will perform the logic actually specific to this
        // application,
        Pes: pes::PesPacketFilter<DumpDemuxContext,PtsDumpElementaryStreamConsumer>,

        // these definitions are boilerplate required by the framework,
        Pat: demultiplex::PatPacketFilter<DumpDemuxContext>,
        Pmt: demultiplex::PmtPacketFilter<DumpDemuxContext>,

        // this variant will be used when we want to ignore data in the transport stream that this
        // application does not care about
        Null: demultiplex::NullPacketFilter<DumpDemuxContext>,
    }
}

// This macro invocation creates a type called DumpDemuxContext, which is our application-specific
// implementation of the DemuxContext trait.
demux_context!(DumpDemuxContext, DumpFilterSwitch);

// When the de-multiplexing process needs to create a PacketFilter instance to handle a particular
// kind of data discovered within the Transport Stream being processed, it will send a
// FilterRequest to our application-specific implementation of the do_construct() method
impl DumpDemuxContext {
    fn do_construct(&mut self, req: demultiplex::FilterRequest<'_, '_>) -> DumpFilterSwitch {
        match req {
            // The 'Program Association Table' is is always on PID 0.  We just use the standard
            // handling here, but an application could insert its own logic if required,
            demultiplex::FilterRequest::ByPid(psi::pat::PAT_PID) => {
                DumpFilterSwitch::Pat(demultiplex::PatPacketFilter::default())
            }
            // 'Stuffing' data on PID 0x1fff may be used to pad-out parts of the transport stream
            // so that it has constant overall bitrate.  This causes it to be ignored if present.
            demultiplex::FilterRequest::ByPid(mpeg2ts_reader::STUFFING_PID) => {
                DumpFilterSwitch::Null(demultiplex::NullPacketFilter::default())
            }
            // Some Transport Streams will contain data on 'well known' PIDs, which are not
            // announced in PAT / PMT metadata.  This application does not process any of these
            // well known PIDs, so we register NullPacketFiltet such that they will be ignored
            demultiplex::FilterRequest::ByPid(_) => {
                DumpFilterSwitch::Null(demultiplex::NullPacketFilter::default())
            }
            // This match-arm installs our application-specific handling for each H264 stream
            // discovered within the transport stream,
            demultiplex::FilterRequest::ByStream {
                stream_type: StreamType::H264,
                pmt,
                stream_info,
                ..
            } => PtsDumpElementaryStreamConsumer::construct(pmt, stream_info),
            // We need to have a match-arm to specify how to handle any other StreamType values
            // that might be present; we answer with NullPacketFilter so that anything other than
            // H264 (handled above) is ignored,
            demultiplex::FilterRequest::ByStream { .. } => {
                DumpFilterSwitch::Null(demultiplex::NullPacketFilter::default())
            }
            // The 'Program Map Table' defines the sub-streams for a particular program within the
            // Transport Stream (it is common for Transport Streams to contain only one program).
            // We just use the standard handling here, but an application could insert its own
            // logic if required,
            demultiplex::FilterRequest::Pmt {
                pid,
                program_number,
            } => DumpFilterSwitch::Pmt(demultiplex::PmtPacketFilter::new(pid, program_number)),
            // Ignore 'Network Information Table', if present,
            demultiplex::FilterRequest::Nit { .. } => {
                DumpFilterSwitch::Null(demultiplex::NullPacketFilter::default())
            }
        }
    }
}

// Implement the ElementaryStreamConsumer to just dump and PTS/DTS timestamps to stdout
pub struct PtsDumpElementaryStreamConsumer {
    nal_parser: NALParser,
    packet_end_ts: f64,
    is_first_packet: bool
}

impl PtsDumpElementaryStreamConsumer {
    fn construct(
        _pmt_sect: &psi::pmt::PmtSection,
        stream_info: &psi::pmt::StreamInfo,
    ) -> DumpFilterSwitch {
        let filter = pes::PesPacketFilter::new(PtsDumpElementaryStreamConsumer {
            nal_parser: NALParser::new(),
            packet_end_ts: 0.0,
            is_first_packet: false
        });
        DumpFilterSwitch::Pes(filter)
    }

    fn time_in_seconds(pts: Timestamp) -> f64 {
        (pts.value() as f64 * 1.0) / (Timestamp::TIMEBASE as f64)
    }
}

impl pes::ElementaryStreamConsumer<DumpDemuxContext> for PtsDumpElementaryStreamConsumer {
    fn start_stream(&mut self, _ctx: &mut DumpDemuxContext) {
        self.is_first_packet = true;
    }

    fn begin_packet(&mut self, _ctx: &mut DumpDemuxContext, header: pes::PesHeader) {
        match header.contents() {
            pes::PesContents::Parsed(Some(parsed)) => {
                match parsed.pts_dts() {
                    Ok(pes::PtsDts::PtsOnly(Ok(pts))) => {
                        if self.is_first_packet {
                            self.nal_parser.decoding_delay(Self::time_in_seconds(pts));
                            self.is_first_packet = false;
                        } else {
                            self.nal_parser.set_duration_for_previous_packet(Self::time_in_seconds(pts));
                        }
                        self.packet_end_ts = Self::time_in_seconds(pts);
                    }
                    _ => (),
                }
                let payload = parsed.payload();
                let mut output_file = File::options().append(true).open("a.dat").expect("test");

                let mut writer = BufWriter::new(output_file);
                // let startCode:[u8;4] = [ 0x00, 0x00, 0x00, 0x01];
                // writer.write(&startCode);
                // writer.write(payload);
                // println!("Is Unit Access Code",)
                self.nal_parser.begin_packet(payload);
                // let accessCode = NALParser::is_four_byte_nalunit(
                //     6,
                //     HeaderCode::Sps,
                //     &payload
                // );
                //
                // if accessCode == true {
                //     println!("Contains SPS");
                // }
                // println!(
                //     "Start write {:02x}",
                //     payload[..cmp::min(payload.len(), 16)].plain_hex(false)
                // )
            }
            pes::PesContents::Parsed(None) => (),
            pes::PesContents::Payload(payload) => {

                // println!(
                //     "......{:?}:                               {:02x}",
                //     self.pid,
                //     payload[..cmp::min(payload.len(), 16)].plain_hex(false)
                // )
            }
        }
    }

    fn continue_packet(&mut self, _ctx: &mut DumpDemuxContext, data: &[u8]) {
        // let output_file = match File::create("a.txt") {
        //     Ok(file) => file,
        //     Err(e) => return (),
        // };
        let mut output_file = File::options().append(true).open("a.dat").expect("test");

        let mut writer = BufWriter::new(output_file);
        // let startCode:[u8;4] = [ 0x00, 0x00, 0x00, 0x01];
        // writer.write(&startCode);
        // writer.write(data);
        self.nal_parser.continue_packet(data);
    }

    fn end_packet(&mut self, _ctx: &mut DumpDemuxContext) {
        self.nal_parser.end_packet(self.packet_end_ts);
    }

    fn continuity_error(&mut self, _ctx: &mut DumpDemuxContext) {}
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    // open input file named on command line,
    let name = env::args().nth(1).unwrap();
    let mut f = File::open(&name).unwrap_or_else(|_| panic!("file not found: {}", &name));

    // create the context object that stores the state of the transport stream demultiplexing
    // process
    let mut ctx = DumpDemuxContext::new();

    // create the demultiplexer, which will use the ctx to create a filter for pid 0 (PAT)
    let mut demux = demultiplex::Demultiplex::new(&mut ctx);
    let mut buf = [0u8; 188 * 1024];

    let buffer: Vec<Vec<i32>> = vec![vec![1, 2, 3], vec![4, 5, 6]];

    // Attempt to create a new file
    // let output_file = match File::create("a.txt") {
    //     Ok(file) => file,
    //     Err(e) => return (),
    // };

    // let mut writer = BufWriter::new(output_file);

    // for chunk in buffer.iter() {
    //     for &item in chunk.iter() {
    //         writer.write_i32::<LittleEndian>(item);
    //     }
    // }


    // for chunk in data {
    //     let test = file.expect("REASON").write_all(&chunk);
    // }

    loop {
        match f.read(&mut buf[..]).expect("read failed") {
            0 => break,
            n => {
                demux.push(&mut ctx, &buf[0..n]);
                // writer.write(&buf);
            }//demux.push(&mut ctx, &buf[0..n]),
        }
    }

    // loop {
    //     f.read(&mut buf[..]).expect("Test");
    //     writer.write(&buf).expect("Test1");
    // }
    // writer.flush();
}
