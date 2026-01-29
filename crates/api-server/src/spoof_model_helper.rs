
/// Returns the Gemini spoof model for a given Anthropic model
fn get_spoof_model(model: AntigravityModel) -> Option<AntigravityModel> {
    match model {
        AntigravityModel::ClaudeOpus45Thinking => Some(AntigravityModel::Gemini3Pro),
        AntigravityModel::ClaudeSonnet45Thinking | AntigravityModel::ClaudeSonnet45 => Some(AntigravityModel::Gemini3Flash),
        // Allow Pro -> Flash fallback
        AntigravityModel::Gemini3Pro => Some(AntigravityModel::Gemini3Flash),
        _ => None,
    }
}
