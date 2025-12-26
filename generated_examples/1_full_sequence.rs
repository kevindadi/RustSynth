use base64::Engine;
use base64::GeneralPurposeConfig;
use base64::EncoderStringWriter;
use base64::EncoderWriter;
use base64::DecoderReader;

#[test]
fn generated_api_sequence() {
    let result_0 = todo!("need Engine instance").decode_vec(/* T */ todo!("input"), &mut Vec::new());
    let self_1 = self_1.with_decode_allow_trailing_bits(true);
    let result_2 = todo!("need Engine instance").encode_slice(/* T */ todo!("input"), &mut &[]);
    let _ = todo!("need Engine instance").encode_string(/* T */ todo!("input"), &mut String::new());
    let self_3 = EncoderStringWriter::new(/* E */ todo!("engine"));
    let encoderwriter_4 = EncoderWriter::new(/* W */ todo!("delegate"), /* E */ todo!("engine"));
    let s_5 = self_3.into_inner();
    let self_6 = self_6.with_encode_padding(true);
    let result_7 = todo!("need Engine instance").decode(/* T */ todo!("input"));
    let self_8 = DecoderReader::new(/* R */ todo!("reader"), /* E */ todo!("engine"));
}
