use std::io::Read;

#[derive(Clone, Copy)]
pub enum HeaderCode {
    Delimiter = 0x09,
    //00 00 00 01 09
    Sps = 0x67,
    // 00 00 00 01 67
    Pps = 0x68,
    // 00 00 00 01 68
    KeyFrame = 0x65,
    // 00 00 01 65
    Sei = 0x06,     // 00 00 01 06
}

impl HeaderCode {
    fn header_prefix_byte_count(&self) -> usize {
        return match self {
            HeaderCode::Sei | HeaderCode::KeyFrame => 3,
            _ => 4,
        };
    }

    fn iframe_header_prefix_byte_count() -> usize {
        return Self::delimiter_length() + 4;
    }

    fn delimiter_length() -> usize {
        return 6;
    }
}

enum VideoFrameType {
    KeyFrame,
    DeltaFrame,
}

enum ParseState {
    Pps,
    Sps,
    Sei,
    KeyFrame,
    PFrame,
}

pub struct DemuxedSegment {
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
    frame_type: VideoFrameType,
    frame_payload: Option<Vec<u8>>,
}

pub struct NALParser {
    state: ParseState,
    raw_data: Option<Vec<u8>>,
    segments: Option<Vec<DemuxedSegment>>,
    sei_found: bool,
}

impl NALParser {
    pub fn new() -> Self {
        return NALParser {
            state: ParseState::Sps,
            raw_data: None,
            segments: Some(Vec::new()),
            sei_found: false,
        };
    }

    pub fn begin_packet(&mut self, payload: &[u8]) {
        self.raw_data = Some(payload.to_vec());
    }

    pub fn continue_packet(&mut self, payload: &[u8]) {
        if let Some(ref mut vec) = self.raw_data {
            vec.extend_from_slice(payload);
        }
    }

    pub fn end_packet(&mut self) {
        if let Some(ref vec) = self.raw_data {
            let sps_header_index = NALParser::index_of_payload(vec,
                                                               HeaderCode::delimiter_length(),
                                                               HeaderCode::Sps);
            let mut active_segment: DemuxedSegment;
            if sps_header_index.start_index.is_some() { // key frame
                active_segment = DemuxedSegment {
                    sps: None,
                    pps: None,
                    frame_type: VideoFrameType::KeyFrame,
                    frame_payload: None,
                };
                let pps_header_index = NALParser::index_of_payload(vec,
                                                                   sps_header_index.end_index.unwrap(),
                                                                   HeaderCode::Pps,);
                if let Some(pps_end_index) = pps_header_index.end_index {
                    active_segment.sps = get_header_payload(&pps_header_index, &vec);
                    Self::print(&pps_header_index, HeaderCode::Sps);
                    let header = if self.sei_found == true { HeaderCode::KeyFrame } else { HeaderCode::Sei };
                    let sei_or_key_frame_index = NALParser::index_of_payload(vec,
                                                                             pps_end_index, header);
                    let key_frame_header_end_index: usize;

                    if self.sei_found == false {

                        if let Some(sei_or_key_frame_end_index) = sei_or_key_frame_index.end_index {
                            active_segment.pps = get_header_payload(&sei_or_key_frame_index, &vec);
                            Self::print(&sei_or_key_frame_index, HeaderCode::Pps);
                        }
                        let key_frame_index = NALParser::index_of_payload(vec,
                                                                          sei_or_key_frame_index.end_index.unwrap_or_else(|| 0),
                                                                          HeaderCode::KeyFrame);
                        key_frame_header_end_index = key_frame_index.end_index.unwrap_or_else(|| 0);
                    } else {
                        active_segment.pps = get_header_payload(&sei_or_key_frame_index, &vec);
                        key_frame_header_end_index = sei_or_key_frame_index.end_index.unwrap_or_else(|| 0);
                        Self::print(&sei_or_key_frame_index, HeaderCode::Pps);
                    }
                    self.sei_found = true;

                    if let Some(key_frame_data) = &self.raw_data {
                        active_segment.frame_payload = Some(vec[key_frame_header_end_index..key_frame_data.len()].to_vec());
                        println!("Packet :KeyFrame Payload size {}", key_frame_data.len() - key_frame_header_end_index);
                    }
                }
            } else { // delta frame
                active_segment = DemuxedSegment {
                    sps: None,
                    pps: None,
                    frame_type: VideoFrameType::DeltaFrame,
                    frame_payload: None,
                };

                let header_size = HeaderCode::iframe_header_prefix_byte_count();
                if let Some(delta_frame_data) = &self.raw_data {
                    active_segment.frame_payload = Some(vec[header_size..delta_frame_data.len()].to_vec());
                    println!("delta frame size {}", delta_frame_data.len() - header_size);
                }
            }
            if let Some(ref mut demux_segments) = self.segments {
                demux_segments.push(active_segment);
            } else {
                self.segments = Some(vec![active_segment]);
            }
        }
    }

    fn print(indices: &HeaderIndices, header_code: HeaderCode) {
        let header = match header_code {
            HeaderCode::Delimiter => "Delimiter",
            HeaderCode::Sps => "SPS",
            HeaderCode::Pps => "PPS",
            HeaderCode::KeyFrame => "KEYFRAME",
            HeaderCode::Sei => "IFRAME",
        };
        println!("Packet :{} Payload size {}", header, (indices.start_index.unwrap_or_else(|| 0)
            - indices.prev_header_end.unwrap_or_else(|| 0)))
    }

    // Return value is Header start , Header end, Previous Header End
    fn index_of_payload(data_stream: &[u8], start_index: usize, header_coder: HeaderCode) -> HeaderIndices {
        match header_coder.header_prefix_byte_count() {
            3 => Self::index_of_three_byte_nal_unit(start_index, header_coder, data_stream),
            4 => Self::index_of_four_byte_nal_unit(start_index, header_coder, data_stream),
            _ => HeaderIndices::new(None, None, None)
        }
    }

    pub fn index_of_four_byte_nal_unit(start_index: usize, header_code: HeaderCode, data_stream: &[u8]) -> HeaderIndices {
        let start = start_index;
        let mut iterator = start_index;
        while iterator < (data_stream.len() - header_code.header_prefix_byte_count()) {
            let byte_zero = data_stream[iterator];
            let byte_one = data_stream[iterator + 1];
            let byte_two = data_stream[iterator + 2];
            let byte_three = data_stream[iterator + 3];
            let byte_four = data_stream[iterator + 4];

            let contains = (byte_zero | byte_one | byte_two | byte_three) == 1 && byte_four == (header_code as u8);
            if contains {
                if Some(start).unwrap() == Some(iterator).unwrap() {
                    return HeaderIndices::new(Some(iterator), Some(iterator + 4), None);
                }
                return HeaderIndices::new(Some(iterator), Some(iterator + 4), Some(start));
            }
            iterator = iterator + 1;
        }
        return HeaderIndices::new(None, None, None);
    }

    pub fn index_of_three_byte_nal_unit(start_index: usize, header_code: HeaderCode, data_stream: &[u8]) -> HeaderIndices {
        let start = start_index;
        let mut iterator = start_index;
        while iterator < (data_stream.len() - header_code.header_prefix_byte_count()) {
            let byte_zero = data_stream[iterator];
            let byte_one = data_stream[iterator + 1];
            let byte_two = data_stream[iterator + 2];
            let byte_three = data_stream[iterator + 3];

            let contains = (byte_zero | byte_one | byte_two) == 1 && byte_three == header_code as u8;
            if contains {
                if Some(start).unwrap_or_else(|| 0) == Some(iterator).unwrap_or_else(|| 0) {
                    return HeaderIndices::new(Some(iterator), Some(iterator + 3), None )
                }
                return HeaderIndices::new(Some(iterator), Some(iterator + 3), Some(start));
            }
            iterator = iterator + 1;
        }
        return HeaderIndices::new(None, None, None);
    }
}

pub struct HeaderIndices {
    start_index : Option<usize>,
    end_index : Option<usize>,
    prev_header_end : Option<usize>,
}

impl HeaderIndices {
    pub fn new(
        start_index: Option<usize>,
        end_index: Option<usize>,
        prev_header_end_index: Option<usize>
    ) -> HeaderIndices {
        HeaderIndices {
            start_index,
            end_index,
            prev_header_end: prev_header_end_index,
        }
    }
}

pub fn get_header_payload(next_header_indices: &HeaderIndices, vec: &Vec<u8>) -> Option<Vec<u8>> {
    if next_header_indices.start_index.is_some() && next_header_indices.prev_header_end.is_some() {
        let slice = &vec[next_header_indices.prev_header_end.unwrap()..next_header_indices.start_index.unwrap()];
        return Some(slice.to_vec())
    }
    None
}