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
    key_frame,
    i_frame,
}

enum ParseState {
    pps,
    sps,
    sei,
    key_frame,
    i_frame,
}

pub struct DemuxedSegment {
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
    frameType: VideoFrameType,
    framePayload: Option<Vec<u8>>,
}

pub struct NALParser {
    state: ParseState,
    activeSegment: DemuxedSegment,
    rawData: Option<Vec<u8>>,
    segments: Option<Vec<DemuxedSegment>>,
    seiFound: bool,
}

impl NALParser {
    pub fn new() -> Self {
        let payload: Vec<u8> = Vec::new();

        return NALParser {
            state: ParseState::sps,
            activeSegment: DemuxedSegment {
                sps: Option::None,
                pps: Option::None,
                frameType: VideoFrameType::key_frame,
                framePayload: None,
            },
            rawData: Option::None,
            segments: Some(Vec::new()),
            seiFound: false,
        };
    }

    pub fn beginPacket(&mut self, payload: &[u8]) {
        self.rawData = Some(payload.to_vec());
    }

    pub fn continuePacket(&mut self, payload: &[u8]) {
        if let Some(ref mut vec) = self.rawData {
            // println!("Break {}",vec.len());

            vec.extend_from_slice(payload);
            // if vec.len() > 1850 {
            //     println!("Break {}",vec.len());
            // }
        }
    }

    pub fn endPacket(&mut self) {
        if let Some(ref vec) = self.rawData {
            let spsHeaderIndex = NALParser::indexOfPayload(vec,
                                                           HeaderCode::delimiter_length(),
                                                           HeaderCode::Sps);
            if spsHeaderIndex.0.is_some() { // key frame
                let ppsHeaderIndex = NALParser::indexOfPayload(vec,
                                                               spsHeaderIndex.1.unwrap_or_else(|| 0),
                                                               HeaderCode::Pps);
                if let Some(ppsEndIndex) = ppsHeaderIndex.1 {
                    Self::print(ppsHeaderIndex, HeaderCode::Sps);
                    let header = if self.seiFound == true { HeaderCode::KeyFrame } else { HeaderCode::Sei };
                    let sei_or_key_frame_index = NALParser::indexOfPayload(vec,
                                                                           ppsEndIndex, header);
                    let mut keyFrameHeaderEndIndex: usize;

                    if self.seiFound == false {
                        if let Some(sei_or_key_frame_end_index) = sei_or_key_frame_index.1 {
                            Self::print(sei_or_key_frame_index, HeaderCode::Pps);
                        }
                        let keyFrameIndex = NALParser::indexOfPayload(vec,
                                                                      sei_or_key_frame_index.1.unwrap_or_else(|| 0),
                                                                      HeaderCode::KeyFrame);
                        keyFrameHeaderEndIndex = keyFrameIndex.1.unwrap_or_else(|| 0);
                    } else {
                        keyFrameHeaderEndIndex = sei_or_key_frame_index.1.unwrap_or_else(|| 0);
                        Self::print(sei_or_key_frame_index, HeaderCode::Pps);
                    }
                    self.seiFound = true;
                    // let keyframePayloadSize:
                    // let iFrameIndex = NALParser::indexOfPayload(vec
                    //     , keyFrameHeaderEndIndex, HeaderCode::Delimiter);
                    // Self::print(iFrameIndex, HeaderCode::KeyFrame);

                    if let Some(keyFrameData) = &self.rawData {
                        println!("Packet :KeyFrame Payload size {}", keyFrameData.len() - keyFrameHeaderEndIndex)
                    }
                    // println!("Packet :KeyFrame Payload size {}",self.rawData.len() - keyFrameHeaderEndIndex )

                    // if let Some( sei_or_key_frame_end_index ) = sei_or_key_frame_index.1 {
                    //     Self::print(sei_or_key_frame_index, HeaderCode::Pps);
                    // }else{

                    // }
                }
            } else { // iframe
                let header_size = HeaderCode::iframe_header_prefix_byte_count();
                if let Some(keyFrameData) = &self.rawData {
                    println!("delta frame size {}", keyFrameData.len() - header_size);
                }
                // iFrame
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

    fn indexOfPayload(data_stream: &[u8], startIndex: usize, header_coder: HeaderCode) -> (Option<usize>, Option<usize>, Option<usize>) {
        match header_coder.header_prefix_byte_count() {
            3 => Self::index_of_three_byte_nalunit(startIndex, header_coder, data_stream),
            4 => Self::index_of_four_byte_nalunit(startIndex, header_coder, data_stream),
            _ => (Option::None, Option::None, Option::None)
        }
    }
    //Return value is Header start , Header end, Previous Header End
    pub fn index_of_four_byte_nalunit(start_index: usize, header_code: HeaderCode, data_stream: &[u8]) -> (Option<usize>, Option<usize>, Option<usize>) {
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
                if Some(start).unwrap_or_else(|| 0) == Some(iterator).unwrap_or_else(|| 0) {
                    return (Some(iterator), Some(iterator + 4), None);
                }
                return (Some(iterator), Some(iterator + 4), Some(start));
            }
            iterator = iterator + 1;
        }
        return (None, None, None);
    }

    pub fn index_of_three_byte_nalunit(start_index: usize, header_code: HeaderCode, data_stream: &[u8]) -> (Option<usize>, Option<usize>, Option<usize>) {
        let start = start_index;
        let mut iterator = start_index;
        let code = header_code as u8;
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