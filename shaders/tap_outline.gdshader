// Tap-scrim outline shader.
// Traces the alpha boundary of the source texture to produce a white border
// with a gray drop shadow.  Used by the TapHighlight system.
//
// Every asset's baked-in outer border (architectural rule) ensures a clean
// alpha edge for this shader to follow.  See docs/foliage_architecture.md.

shader_type canvas_item;

uniform vec4 outline_color : source_color = vec4(1.0, 1.0, 1.0, 1.0);
uniform vec4 shadow_color  : source_color = vec4(0.5, 0.5, 0.5, 0.6);
uniform float outline_width : hint_range(0.5, 8.0) = 2.0;
uniform float shadow_width  : hint_range(0.5, 10.0) = 3.5;
uniform vec2  shadow_offset = vec2(1.5, 1.5);

void fragment() {
	vec2 ps = TEXTURE_PIXEL_SIZE;
	float center_alpha = texture(TEXTURE, UV).a;

	float outline_alpha = 0.0;
	float shadow_alpha  = 0.0;

	// 16 evenly-spaced samples around the pixel — smooth enough for mobile.
	for (int i = 0; i < 16; i++) {
		float angle = float(i) * 6.2831853 / 16.0;
		vec2 dir = vec2(cos(angle), sin(angle));

		// Outline ring at outline_width distance.
		vec2 o_uv = UV + dir * outline_width * ps;
		outline_alpha = max(outline_alpha, texture(TEXTURE, o_uv).a);

		// Shadow ring — larger radius, shifted down-right.
		vec2 s_uv = UV + dir * shadow_width * ps + shadow_offset * ps;
		shadow_alpha = max(shadow_alpha, texture(TEXTURE, s_uv).a);
	}

	if (center_alpha > 0.1) {
		// Inside the asset shape — fully transparent so the original shows through.
		COLOR = vec4(0.0);
	} else if (outline_alpha > 0.1) {
		COLOR = outline_color;
	} else if (shadow_alpha > 0.1) {
		COLOR = shadow_color;
	} else {
		COLOR = vec4(0.0);
	}
}
