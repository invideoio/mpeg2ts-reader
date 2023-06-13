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
    IFrame,
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
    active_segment: DemuxedSegment,
    raw_data: Option<Vec<u8>>,
    segments: Option<Vec<DemuxedSegment>>,
    sei_found: bool,
}

impl NALParser {
    pub fn new() -> Self {
        return NALParser {
            state: ParseState::Sps,
            active_segment: DemuxedSegment {
                sps: None,
                pps: None,
                frame_type: VideoFrameType::KeyFrame,
                frame_payload: None,
            },
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
            if sps_header_index.0.is_some() { // key frame
                let pps_header_index = NALParser::index_of_payload(vec,
                                                                   sps_header_index.1.unwrap_or_else(|| 0),
                                                                   HeaderCode::Pps,);
                if let Some(pps_end_index) = pps_header_index.1 {
                    Self::print(pps_header_index, HeaderCode::Sps);
                    let header = if self.sei_found == true { HeaderCode::KeyFrame } else { HeaderCode::Sei };
                    let sei_or_key_frame_index = NALParser::index_of_payload(vec,
                                                                             pps_end_index, header);
                    let key_frame_header_end_index: usize;

                    if self.sei_found == false {
                        if let Some(sei_or_key_frame_end_index) = sei_or_key_frame_index.1 {
                            Self::print(sei_or_key_frame_index, HeaderCode::Pps);
                        }
                        let key_frame_index = NALParser::index_of_payload(vec,
                                                                          sei_or_key_frame_index.1.unwrap_or_else(|| 0),
                                                                          HeaderCode::KeyFrame);
                        key_frame_header_end_index = key_frame_index.1.unwrap_or_else(|| 0);
                    } else {
                        key_frame_header_end_index = sei_or_key_frame_index.1.unwrap_or_else(|| 0);
                        Self::print(sei_or_key_frame_index, HeaderCode::Pps);
                    }
                    self.sei_found = true;

                    if let Some(key_frame_data) = &self.raw_data {
                        println!("Packet :KeyFrame Payload size {}", key_frame_data.len() - key_frame_header_end_index)
                    }
                }
            } else { // delta frame
                let header_size = HeaderCode::iframe_header_prefix_byte_count();
                if let Some(keyFrameData) = &self.raw_data {
                    println!("delta frame size {}", keyFrameData.len() - header_size);
                }
            }
        }
    }

    fn print(index: (Option<usize>, Option<usize>, Option<usize>), header_code: HeaderCode) {
        let header = match header_code {
            HeaderCode::Delimiter => "Delimiter",
            HeaderCode::Sps => "SPS",
            HeaderCode::Pps => "PPS",
            HeaderCode::KeyFrame => "KEYFRAME",
            HeaderCode::Sei => "IFRAME",
        };
        println!("Packet :{} Payload size {}", header, (index.0.unwrap_or_else(|| 0)
            - index.2.unwrap_or_else(|| 0)))
    }

    // Return value is Header start , Header end, Previous Header End
    fn index_of_payload(data_stream: &[u8], start_index: usize, header_coder: HeaderCode) -> (Option<usize>, Option<usize>, Option<usize>) {
        match header_coder.header_prefix_byte_count() {
            3 => Self::index_of_three_byte_nal_unit(start_index, header_coder, data_stream),
            4 => Self::index_of_four_byte_nal_unit(start_index, header_coder, data_stream),
            _ => (None, None, None)
        }
    }

    pub fn index_of_four_byte_nal_unit(start_index: usize, header_code: HeaderCode, data_stream: &[u8]) -> (Option<usize>, Option<usize>, Option<usize>) {
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
                    return (Some(iterator), Some(iterator + 4), None);
                }
                return (Some(iterator), Some(iterator + 4), Some(start));
            }
            iterator = iterator + 1;
        }
        return (None, None, None);
    }

    pub fn index_of_three_byte_nal_unit(start_index: usize, header_code: HeaderCode, data_stream: &[u8]) -> (Option<usize>, Option<usize>, Option<usize>) {
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
                    return (Some(iterator), Some(iterator + 3), None);
                }
                return (Some(iterator), Some(iterator + 3), Some(start));
            }
            iterator = iterator + 1;
        }
        return (None, None, None);
    }
}
