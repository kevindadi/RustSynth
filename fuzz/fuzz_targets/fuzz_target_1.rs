#![no_main]
use libfuzzer_sys::fuzz_target;
use arbitrary::Arbitrary;
use base64;

#[derive(Default)]
struct FuzzState {
    pool_tuple_: Vec<()>,
    pool_Alphabet: Vec<Alphabet>,
    pool_Base64Display: Vec<Base64Display>,
    pool_Config: Vec<Config>,
    pool_DecodeError: Vec<DecodeError>,
    pool_DecodePaddingMode: Vec<DecodePaddingMode>,
    pool_DecodeSliceError: Vec<DecodeSliceError>,
    pool_DecoderReader: Vec<DecoderReader>,
    pool_EncodeSliceError: Vec<EncodeSliceError>,
    pool_EncoderStringWriter: Vec<EncoderStringWriter>,
    pool_EncoderWriter: Vec<EncoderWriter>,
    pool_GeneralPurpose: Vec<GeneralPurpose>,
    pool_GeneralPurposeConfig: Vec<GeneralPurposeConfig>,
    pool_ParseAlphabetError: Vec<ParseAlphabetError>,
    pool_String: Vec<String>,
    pool_Vec_u8: Vec<Vec<u8>>,
    pool_array_u8: Vec<[u8]>,
}

#[derive(Arbitrary, Debug)]
enum Action {
    Get_CRYPT {
    },
    Get_URL_SAFE_NO_PAD {
    },
    Get_URL_SAFE_NO_PAD_INDIFFERENT {
    },
    Get_STANDARD_NO_PAD_INDIFFERENT {
    },
    Get_NO_PAD_INDIFFERENT {
    },
    Get_STANDARD_PAD_INDIFFERENT {
    },
    Get_BCRYPT {
    },
    Get_STANDARD {
    },
    Get_STANDARD_NO_PAD {
    },
    Get_PAD {
    },
    Get_PAD_INDIFFERENT {
    },
    Get_IMAP_MUTF7 {
    },
    Get_URL_SAFE_PAD_INDIFFERENT {
    },
    Get_STANDARD {
    },
    Get_NO_PAD {
    },
    Get_BIN_HEX {
    },
    Get_URL_SAFE {
    },
    Get_URL_SAFE {
    },
    Consume {
        arg0: str,
    },
    Encode_engine_string {
        arg1_idx: usize,
    },
    Decode_engine {
    },
    Decoded_len_estimate {
        arg0: usize,
    },
    Encoded_len {
        arg0: usize,
        arg1: bool,
    },
    Encode_engine_slice {
        arg1_idx: usize,
    },
    Encode {
    },
    Encode_engine {
    },
    Decode_engine_slice {
        arg1_idx: usize,
    },
    Decode {
    },
    Decode_engine_vec {
        arg1_idx: usize,
    },
    DecodeErrorInvalidByte {
    },
    DecodeErrorInvalidLength {
    },
    DecodeErrorInvalidLastSymbol {
    },
    DecodeErrorInvalidPadding {
    },
    ParseAlphabetErrorInvalidLength {
    },
    ParseAlphabetErrorDuplicatedByte {
    },
    ParseAlphabetErrorUnprintableByte {
    },
    ParseAlphabetErrorReservedByte {
    },
    EncodeSliceErrorOutputSliceTooSmall {
    },
    DecodePaddingModeIndifferent {
    },
    DecodePaddingModeRequireCanonical {
    },
    DecodePaddingModeRequireNone {
    },
    DecodeSliceErrorDecodeError {
    },
    DecodeSliceErrorOutputSliceTooSmall {
    },
    New {
    },
    Into_inner {
        arg0_idx: usize,
    },
    Write {
        arg0_idx: usize,
        arg1_idx: usize,
    },
    Flush {
        arg0_idx: usize,
    },
    New {
    },
    With_encode_padding {
        arg0_idx: usize,
        arg1: bool,
    },
    With_decode_allow_trailing_bits {
        arg0_idx: usize,
        arg1: bool,
    },
    With_decode_padding_mode {
        arg0_idx: usize,
        arg1_idx: usize,
    },
    New {
    },
    Finish {
        arg0_idx: usize,
    },
    Into_inner {
        arg0_idx: usize,
    },
    New {
    },
    Read {
        arg0_idx: usize,
        arg1_idx: usize,
    },
    New {
        arg0_idx: usize,
        arg1_idx: usize,
    },
    Encode_padding {
        arg0_idx: usize,
    },
    From_consumer {
    },
    Into_inner {
        arg0_idx: usize,
    },
    New {
        arg0: str,
    },
    As_str {
        arg0_idx: usize,
    },
    New {
        arg0_idx: usize,
    },
    Write {
        arg0_idx: usize,
        arg1_idx: usize,
    },
    Flush {
        arg0_idx: usize,
    },
    Config {
        arg0_idx: usize,
    },
    Consume {
        arg0_idx: usize,
        arg1: str,
    },
}

fuzz_target!(|actions: Vec<Action>| {
    let mut state = FuzzState::default();
    for action in actions {
        match action {
            Action::Get_CRYPT { } => {
                let res = get_CRYPT();
                state.pool_Alphabet.push(res);
            }
            Action::Get_URL_SAFE_NO_PAD { } => {
                let res = get_URL_SAFE_NO_PAD();
                state.pool_GeneralPurpose.push(res);
            }
            Action::Get_URL_SAFE_NO_PAD_INDIFFERENT { } => {
                let res = get_URL_SAFE_NO_PAD_INDIFFERENT();
                state.pool_GeneralPurpose.push(res);
            }
            Action::Get_STANDARD_NO_PAD_INDIFFERENT { } => {
                let res = get_STANDARD_NO_PAD_INDIFFERENT();
                state.pool_GeneralPurpose.push(res);
            }
            Action::Get_NO_PAD_INDIFFERENT { } => {
                let res = get_NO_PAD_INDIFFERENT();
                state.pool_GeneralPurposeConfig.push(res);
            }
            Action::Get_STANDARD_PAD_INDIFFERENT { } => {
                let res = get_STANDARD_PAD_INDIFFERENT();
                state.pool_GeneralPurpose.push(res);
            }
            Action::Get_BCRYPT { } => {
                let res = get_BCRYPT();
                state.pool_Alphabet.push(res);
            }
            Action::Get_STANDARD { } => {
                let res = get_STANDARD();
                state.pool_GeneralPurpose.push(res);
            }
            Action::Get_STANDARD_NO_PAD { } => {
                let res = get_STANDARD_NO_PAD();
                state.pool_GeneralPurpose.push(res);
            }
            Action::Get_PAD { } => {
                let res = get_PAD();
                state.pool_GeneralPurposeConfig.push(res);
            }
            Action::Get_PAD_INDIFFERENT { } => {
                let res = get_PAD_INDIFFERENT();
                state.pool_GeneralPurposeConfig.push(res);
            }
            Action::Get_IMAP_MUTF7 { } => {
                let res = get_IMAP_MUTF7();
                state.pool_Alphabet.push(res);
            }
            Action::Get_URL_SAFE_PAD_INDIFFERENT { } => {
                let res = get_URL_SAFE_PAD_INDIFFERENT();
                state.pool_GeneralPurpose.push(res);
            }
            Action::Get_STANDARD { } => {
                let res = get_STANDARD();
                state.pool_Alphabet.push(res);
            }
            Action::Get_NO_PAD { } => {
                let res = get_NO_PAD();
                state.pool_GeneralPurposeConfig.push(res);
            }
            Action::Get_BIN_HEX { } => {
                let res = get_BIN_HEX();
                state.pool_Alphabet.push(res);
            }
            Action::Get_URL_SAFE { } => {
                let res = get_URL_SAFE();
                state.pool_Alphabet.push(res);
            }
            Action::Get_URL_SAFE { } => {
                let res = get_URL_SAFE();
                state.pool_GeneralPurpose.push(res);
            }
            Action::Consume { arg0, } => {
                consume(arg0);
            }
            Action::Encode_engine_string { arg1_idx, } => {
                if arg1_idx >= state.pool_String.len() { continue; }
                encode_engine_string(&mut state.pool_String[arg1_idx]);
            }
            Action::Decode_engine { } => {
                match decode_engine() {
                    Ok(res) => state.pool_Vec_u8.push(res),
                    Err(err) => state.pool_DecodeError.push(err),
                }
            }
            Action::Decoded_len_estimate { arg0, } => {
                decoded_len_estimate(arg0);
            }
            Action::Encoded_len { arg0, arg1, } => {
                encoded_len(arg0, arg1);
            }
            Action::Encode_engine_slice { arg1_idx, } => {
                if arg1_idx >= state.pool_array_u8.len() { continue; }
                match encode_engine_slice(&mut state.pool_array_u8[arg1_idx]) {
                    Ok(_) => {},
                    Err(err) => state.pool_EncodeSliceError.push(err),
                }
            }
            Action::Encode { } => {
                let res = encode();
                state.pool_String.push(res);
            }
            Action::Encode_engine { } => {
                let res = encode_engine();
                state.pool_String.push(res);
            }
            Action::Decode_engine_slice { arg1_idx, } => {
                if arg1_idx >= state.pool_array_u8.len() { continue; }
                match decode_engine_slice(&mut state.pool_array_u8[arg1_idx]) {
                    Ok(_) => {},
                    Err(err) => state.pool_DecodeSliceError.push(err),
                }
            }
            Action::Decode { } => {
                match decode() {
                    Ok(res) => state.pool_Vec_u8.push(res),
                    Err(err) => state.pool_DecodeError.push(err),
                }
            }
            Action::Decode_engine_vec { arg1_idx, } => {
                if arg1_idx >= state.pool_Vec_u8.len() { continue; }
                match decode_engine_vec(&mut state.pool_Vec_u8[arg1_idx]) {
                    Ok(res) => state.pool_tuple_.push(res),
                    Err(err) => state.pool_DecodeError.push(err),
                }
            }
            Action::DecodeErrorInvalidByte { } => {
                let res = DecodeError::InvalidByte();
                state.pool_DecodeError.push(res);
            }
            Action::DecodeErrorInvalidLength { } => {
                let res = DecodeError::InvalidLength();
                state.pool_DecodeError.push(res);
            }
            Action::DecodeErrorInvalidLastSymbol { } => {
                let res = DecodeError::InvalidLastSymbol();
                state.pool_DecodeError.push(res);
            }
            Action::DecodeErrorInvalidPadding { } => {
                let res = DecodeError::InvalidPadding();
                state.pool_DecodeError.push(res);
            }
            Action::ParseAlphabetErrorInvalidLength { } => {
                let res = ParseAlphabetError::InvalidLength();
                state.pool_ParseAlphabetError.push(res);
            }
            Action::ParseAlphabetErrorDuplicatedByte { } => {
                let res = ParseAlphabetError::DuplicatedByte();
                state.pool_ParseAlphabetError.push(res);
            }
            Action::ParseAlphabetErrorUnprintableByte { } => {
                let res = ParseAlphabetError::UnprintableByte();
                state.pool_ParseAlphabetError.push(res);
            }
            Action::ParseAlphabetErrorReservedByte { } => {
                let res = ParseAlphabetError::ReservedByte();
                state.pool_ParseAlphabetError.push(res);
            }
            Action::EncodeSliceErrorOutputSliceTooSmall { } => {
                let res = EncodeSliceError::OutputSliceTooSmall();
                state.pool_EncodeSliceError.push(res);
            }
            Action::DecodePaddingModeIndifferent { } => {
                let res = DecodePaddingMode::Indifferent();
                state.pool_DecodePaddingMode.push(res);
            }
            Action::DecodePaddingModeRequireCanonical { } => {
                let res = DecodePaddingMode::RequireCanonical();
                state.pool_DecodePaddingMode.push(res);
            }
            Action::DecodePaddingModeRequireNone { } => {
                let res = DecodePaddingMode::RequireNone();
                state.pool_DecodePaddingMode.push(res);
            }
            Action::DecodeSliceErrorDecodeError { } => {
                let res = DecodeSliceError::DecodeError();
                state.pool_DecodeSliceError.push(res);
            }
            Action::DecodeSliceErrorOutputSliceTooSmall { } => {
                let res = DecodeSliceError::OutputSliceTooSmall();
                state.pool_DecodeSliceError.push(res);
            }
            Action::New { } => {
                let res = new();
                state.pool_DecoderReader.push(res);
            }
            Action::Into_inner { arg0_idx, } => {
                if arg0_idx >= state.pool_DecoderReader.len() { continue; }
                into_inner(state.pool_DecoderReader.remove(arg0_idx));
            }
            Action::Write { arg0_idx, arg1_idx, } => {
                if arg0_idx >= state.pool_EncoderStringWriter.len() { continue; }
                if arg1_idx >= state.pool_array_u8.len() { continue; }
                write(&mut state.pool_EncoderStringWriter[arg0_idx], &state.pool_array_u8[arg1_idx]);
            }
            Action::Flush { arg0_idx, } => {
                if arg0_idx >= state.pool_EncoderStringWriter.len() { continue; }
                let res = flush(&mut state.pool_EncoderStringWriter[arg0_idx]);
                state.pool_tuple_.push(res);
            }
            Action::New { } => {
                let res = new();
                state.pool_GeneralPurposeConfig.push(res);
            }
            Action::With_encode_padding { arg0_idx, arg1, } => {
                if arg0_idx >= state.pool_GeneralPurposeConfig.len() { continue; }
                let res = with_encode_padding(state.pool_GeneralPurposeConfig.remove(arg0_idx), arg1);
                state.pool_GeneralPurposeConfig.push(res);
            }
            Action::With_decode_allow_trailing_bits { arg0_idx, arg1, } => {
                if arg0_idx >= state.pool_GeneralPurposeConfig.len() { continue; }
                let res = with_decode_allow_trailing_bits(state.pool_GeneralPurposeConfig.remove(arg0_idx), arg1);
                state.pool_GeneralPurposeConfig.push(res);
            }
            Action::With_decode_padding_mode { arg0_idx, arg1_idx, } => {
                if arg0_idx >= state.pool_GeneralPurposeConfig.len() { continue; }
                if arg1_idx >= state.pool_DecodePaddingMode.len() { continue; }
                let res = with_decode_padding_mode(state.pool_GeneralPurposeConfig.remove(arg0_idx), state.pool_DecodePaddingMode.remove(arg1_idx));
                state.pool_GeneralPurposeConfig.push(res);
            }
            Action::New { } => {
                let res = new();
                state.pool_EncoderWriter.push(res);
            }
            Action::Finish { arg0_idx, } => {
                if arg0_idx >= state.pool_EncoderWriter.len() { continue; }
                finish(&mut state.pool_EncoderWriter[arg0_idx]);
            }
            Action::Into_inner { arg0_idx, } => {
                if arg0_idx >= state.pool_EncoderWriter.len() { continue; }
                into_inner(state.pool_EncoderWriter.remove(arg0_idx));
            }
            Action::New { } => {
                let res = new();
                state.pool_EncoderStringWriter.push(res);
            }
            Action::Read { arg0_idx, arg1_idx, } => {
                if arg0_idx >= state.pool_DecoderReader.len() { continue; }
                if arg1_idx >= state.pool_array_u8.len() { continue; }
                read(&mut state.pool_DecoderReader[arg0_idx], &mut state.pool_array_u8[arg1_idx]);
            }
            Action::New { arg0_idx, arg1_idx, } => {
                if arg0_idx >= state.pool_Alphabet.len() { continue; }
                if arg1_idx >= state.pool_GeneralPurposeConfig.len() { continue; }
                let res = new(&state.pool_Alphabet[arg0_idx], state.pool_GeneralPurposeConfig.remove(arg1_idx));
                state.pool_GeneralPurpose.push(res);
            }
            Action::Encode_padding { arg0_idx, } => {
                if arg0_idx >= state.pool_GeneralPurposeConfig.len() { continue; }
                encode_padding(&state.pool_GeneralPurposeConfig[arg0_idx]);
            }
            Action::From_consumer { } => {
                let res = from_consumer();
                state.pool_EncoderStringWriter.push(res);
            }
            Action::Into_inner { arg0_idx, } => {
                if arg0_idx >= state.pool_EncoderStringWriter.len() { continue; }
                into_inner(state.pool_EncoderStringWriter.remove(arg0_idx));
            }
            Action::New { arg0, } => {
                match new(arg0) {
                    Ok(res) => state.pool_Alphabet.push(res),
                    Err(err) => state.pool_ParseAlphabetError.push(err),
                }
            }
            Action::As_str { arg0_idx, } => {
                if arg0_idx >= state.pool_Alphabet.len() { continue; }
                as_str(&state.pool_Alphabet[arg0_idx]);
            }
            Action::New { arg0_idx, } => {
                if arg0_idx >= state.pool_array_u8.len() { continue; }
                let res = new(&state.pool_array_u8[arg0_idx]);
                state.pool_Base64Display.push(res);
            }
            Action::Write { arg0_idx, arg1_idx, } => {
                if arg0_idx >= state.pool_EncoderWriter.len() { continue; }
                if arg1_idx >= state.pool_array_u8.len() { continue; }
                write(&mut state.pool_EncoderWriter[arg0_idx], &state.pool_array_u8[arg1_idx]);
            }
            Action::Flush { arg0_idx, } => {
                if arg0_idx >= state.pool_EncoderWriter.len() { continue; }
                let res = flush(&mut state.pool_EncoderWriter[arg0_idx]);
                state.pool_tuple_.push(res);
            }
            Action::Config { arg0_idx, } => {
                if arg0_idx >= state.pool_GeneralPurpose.len() { continue; }
                let res = config(&state.pool_GeneralPurpose[arg0_idx]);
                state.pool_Config.push(res);
            }
            Action::Consume { arg0_idx, arg1, } => {
                if arg0_idx >= state.pool_String.len() { continue; }
                consume(&mut state.pool_String[arg0_idx], arg1);
            }
        }
    }
});
