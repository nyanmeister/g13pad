// SPDX-License-Identifier: GPL-3.0-or-later
// g13pad-obs: an OBS source that draws the G13 from the driver's state files, so the pad
// is an ordinary source beside the input-overlay ones (no window to capture). It reads
// the same sheet and layout as `g13map obs` (assets/obs: a PNG with every key's sprite
// and its pressed twin 3 px below, a JSON placing them), the driver's `g13-0_keys`
// (stick, backlight, keys held) and `g13-0_lcd` (the frame on the glass), and the
// glass table (~/.config/g13map/glass) for the LCD's colours.
#include <obs-module.h>
#include <graphics/image-file.h>
#include <util/platform.h>
#include <util/dstr.h>
#include <jansson.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

OBS_DECLARE_MODULE()
OBS_MODULE_USE_DEFAULT_LOCALE("g13pad-obs", "en-US")

#define LCD_W 160
#define LCD_H 43
#define LCD_BYTES 960
#define PRESSED_GAP 3
#define MAX_KEYS 48
#define KEY_LEN 16
#define POLL_SECONDS 0.01f
#define GLASS_SECONDS 2.0f
#define LIT 0.55f

#ifndef G13PAD_OBS_SHEET_DIR
#define G13PAD_OBS_SHEET_DIR "/usr/share/g13pad/obs"
#endif

enum kind { KIND_TEXTURE = 0, KIND_KEY = 1, KIND_STICK = 5 };

struct element {
	char id[24];
	int type;
	float x, y, sx, sy, w, h, radius;
};

struct glass_entry {
	uint8_t led[3], shown[3];
};

struct g13 {
	obs_source_t *source;
	char *sheet_dir;
	bool lcd_only;
	int lcd_scale;

	gs_image_file4_t sheet;
	bool sheet_loaded;
	struct element *elements;
	size_t count;
	uint32_t width, height;

	float since_poll, since_glass;
	char keys[MAX_KEYS][KEY_LEN];
	size_t nkeys;
	uint8_t stick[2];
	uint8_t backlight[3];
	char last_keys[1024];
	uint8_t lcd[LCD_BYTES];
	bool have_lcd;
	gs_texture_t *lcd_tex;
	uint8_t lcd_rgba[LCD_W * LCD_H * 4];
	bool lcd_dirty;

	struct glass_entry *glass;
	size_t nglass;
	time_t glass_mtime;
	// From the same file: how far lit pixels sit toward white, and the fit (the
	// linear-light monitor colour of each LED alone) for colours not in the table.
	float lit;
	float fit[9];
	bool have_fit;
};

// ---- paths, mirroring g13map's rules ----

static void config_dir(struct dstr *out)
{
	const char *own = getenv("G13MAP_CONFIG");
	const char *xdg = getenv("XDG_CONFIG_HOME");
	const char *home = getenv("HOME");
	if (own && *own)
		dstr_copy(out, own);
	else if (xdg && *xdg)
		dstr_printf(out, "%s/g13map", xdg);
	else
		dstr_printf(out, "%s/.config/g13map", home ? home : "");
}

static void state_path(struct dstr *out, const char *suffix)
{
	const char *pipe = getenv("G13MAP_PIPE");
	dstr_printf(out, "%s_%s", pipe && *pipe ? pipe : "/run/g13d/g13-0", suffix);
}

// ---- the glass table ----

static void glass_load(struct g13 *s)
{
	struct dstr path = {0};
	config_dir(&path);
	dstr_cat(&path, "/glass");
	struct stat st;
	time_t mtime = os_stat(path.array, &st) == 0 ? st.st_mtime : 0;
	if (mtime == s->glass_mtime && (s->glass || mtime == 0)) {
		dstr_free(&path);
		return;
	}
	s->glass_mtime = mtime;
	bfree(s->glass);
	s->glass = NULL;
	s->nglass = 0;
	s->lit = LIT;
	s->have_fit = false;
	char *text = mtime ? os_quick_read_utf8_file(path.array) : NULL;
	if (!text) {
		// The built-in pairs, the README GIF's: green and orange as the glass shows them.
		s->glass = bzalloc(2 * sizeof(struct glass_entry));
		s->glass[0] = (struct glass_entry){{0, 255, 0}, {0, 150, 0}};
		s->glass[1] = (struct glass_entry){{255, 48, 0}, {255, 128, 0}};
		s->nglass = 2;
	} else {
		size_t cap = 8;
		s->glass = bzalloc(cap * sizeof(struct glass_entry));
		char *save = NULL;
		for (char *line = strtok_r(text, "\n", &save); line; line = strtok_r(NULL, "\n", &save)) {
			char *hash = strchr(line, '#');
			if (hash)
				*hash = 0;
			float f[9];
			if (sscanf(line, " lit %f", &f[0]) == 1) {
				if (f[0] >= 0.0f && f[0] <= 1.0f)
					s->lit = f[0];
				continue;
			}
			if (sscanf(line, " fit %f %f %f %f %f %f %f %f %f", &f[0], &f[1], &f[2], &f[3], &f[4],
				   &f[5], &f[6], &f[7], &f[8]) == 9) {
				bool finite = true;
				for (int i = 0; i < 9; i++)
					finite = finite && isfinite(f[i]);
				if (finite) {
					memcpy(s->fit, f, sizeof f);
					s->have_fit = true;
				}
				continue;
			}
			int v[6];
			if (sscanf(line, "%d %d %d %d %d %d", &v[0], &v[1], &v[2], &v[3], &v[4], &v[5]) != 6)
				continue;
			bool ok = true;
			for (int i = 0; i < 6; i++)
				ok = ok && v[i] >= 0 && v[i] <= 255;
			if (!ok)
				continue;
			if (s->nglass == cap) {
				cap *= 2;
				s->glass = brealloc(s->glass, cap * sizeof(struct glass_entry));
			}
			struct glass_entry *e = &s->glass[s->nglass++];
			for (int i = 0; i < 3; i++) {
				e->led[i] = (uint8_t)v[i];
				e->shown[i] = (uint8_t)v[3 + i];
			}
		}
		bfree(text);
	}
	dstr_free(&path);
	s->lcd_dirty = s->have_lcd;
}

// Linear light to an sRGB byte, clipped to the monitor's range (as g13map's glass.rs).
static uint8_t to_srgb(float l)
{
	l = l < 0.0f ? 0.0f : l > 1.0f ? 1.0f : l;
	float c = l <= 0.0031308f ? 12.92f * l : 1.055f * powf(l, 1.0f / 2.4f) - 0.055f;
	return (uint8_t)(c * 255.0f + 0.5f);
}

// Matched, else through the fit (an LED value is linear light; the LEDs add), else as it is.
static void glass_shown(const struct g13 *s, const uint8_t led[3], uint8_t out[3])
{
	memcpy(out, led, 3);
	for (size_t i = 0; i < s->nglass; i++) {
		if (memcmp(s->glass[i].led, led, 3) == 0) {
			memcpy(out, s->glass[i].shown, 3);
			return;
		}
	}
	if (!s->have_fit)
		return;
	for (int c = 0; c < 3; c++) {
		float l = 0.0f;
		for (int k = 0; k < 3; k++)
			l += s->fit[c * 3 + k] * (led[k] / 255.0f);
		out[c] = to_srgb(l);
	}
}

// The frame as the glass shows it: the backlight colour, lit pixels tinted toward white.
static void compose_lcd(struct g13 *s)
{
	uint8_t bg[3], lit[3];
	glass_shown(s, s->backlight, bg);
	for (int i = 0; i < 3; i++)
		lit[i] = (uint8_t)(bg[i] + (255.0f - bg[i]) * s->lit + 0.5f);
	for (int y = 0; y < LCD_H; y++) {
		for (int x = 0; x < LCD_W; x++) {
			bool on = (s->lcd[x + (y / 8) * LCD_W] >> (y % 8)) & 1;
			uint8_t *p = &s->lcd_rgba[(y * LCD_W + x) * 4];
			memcpy(p, on ? lit : bg, 3);
			p[3] = 255;
		}
	}
	s->lcd_dirty = true;
}

// ---- the daemon's state files ----

static void parse_keys(struct g13 *s, char *text)
{
	s->nkeys = 0;
	char *save = NULL;
	for (char *line = strtok_r(text, "\n", &save); line; line = strtok_r(NULL, "\n", &save)) {
		int a, b, c;
		if (sscanf(line, "stick %d %d", &a, &b) == 2 && a >= 0 && a <= 255 && b >= 0 && b <= 255) {
			s->stick[0] = (uint8_t)a;
			s->stick[1] = (uint8_t)b;
		} else if (sscanf(line, "backlight %d %d %d", &a, &b, &c) == 3 && a >= 0 && a <= 255 && b >= 0 &&
			   b <= 255 && c >= 0 && c <= 255) {
			s->backlight[0] = (uint8_t)a;
			s->backlight[1] = (uint8_t)b;
			s->backlight[2] = (uint8_t)c;
		} else if (strncmp(line, "keys", 4) == 0) {
			char *ws = NULL;
			for (char *w = strtok_r(line + 4, " \t\r", &ws); w && s->nkeys < MAX_KEYS;
			     w = strtok_r(NULL, " \t\r", &ws)) {
				strncpy(s->keys[s->nkeys], w, KEY_LEN - 1);
				s->keys[s->nkeys][KEY_LEN - 1] = 0;
				s->nkeys++;
			}
		}
	}
}

static bool held(const struct g13 *s, const char *id)
{
	for (size_t i = 0; i < s->nkeys; i++)
		if (strcmp(s->keys[i], id) == 0)
			return true;
	return false;
}

static void poll_state(struct g13 *s)
{
	struct dstr path = {0};
	state_path(&path, "keys");
	char *text = os_quick_read_utf8_file(path.array);
	if (!text) {
		if (s->last_keys[0]) {
			s->last_keys[0] = 0;
			s->nkeys = 0;
			s->stick[0] = s->stick[1] = 128;
		}
	} else if (strncmp(text, s->last_keys, sizeof(s->last_keys) - 1) != 0) {
		strncpy(s->last_keys, text, sizeof(s->last_keys) - 1);
		s->last_keys[sizeof(s->last_keys) - 1] = 0;
		uint8_t old[3];
		memcpy(old, s->backlight, 3);
		parse_keys(s, text);
		if (memcmp(old, s->backlight, 3) != 0 && s->have_lcd)
			compose_lcd(s);
	}
	bfree(text);

	state_path(&path, "lcd");
	FILE *f = os_fopen(path.array, "rb");
	if (f) {
		uint8_t frame[LCD_BYTES];
		size_t n = fread(frame, 1, LCD_BYTES, f);
		fclose(f);
		if (n == LCD_BYTES && (!s->have_lcd || memcmp(frame, s->lcd, LCD_BYTES) != 0)) {
			memcpy(s->lcd, frame, LCD_BYTES);
			s->have_lcd = true;
			compose_lcd(s);
		}
	} else {
		s->have_lcd = false;
	}
	dstr_free(&path);
}

// ---- the sheet and its layout ----

static void sheet_free(struct g13 *s)
{
	obs_enter_graphics();
	if (s->sheet_loaded)
		gs_image_file4_free(&s->sheet);
	obs_leave_graphics();
	s->sheet_loaded = false;
	bfree(s->elements);
	s->elements = NULL;
	s->count = 0;
	s->width = s->height = 0;
}

static bool number_at(json_t *array, size_t i, float *out)
{
	json_t *v = json_array_get(array, i);
	if (!json_is_number(v))
		return false;
	*out = (float)json_number_value(v);
	return true;
}

static void sheet_dir(struct g13 *s, struct dstr *out)
{
	if (s->sheet_dir && *s->sheet_dir) {
		dstr_copy(out, s->sheet_dir);
		return;
	}
	config_dir(out);
	dstr_cat(out, "/obs");
	struct dstr probe = {0};
	dstr_printf(&probe, "%s/g13.json", out->array);
	bool own = os_file_exists(probe.array);
	dstr_free(&probe);
	if (!own)
		dstr_copy(out, G13PAD_OBS_SHEET_DIR);
}

static void sheet_load(struct g13 *s)
{
	sheet_free(s);
	struct dstr dir = {0}, png = {0}, layout = {0};
	sheet_dir(s, &dir);
	dstr_printf(&png, "%s/g13.png", dir.array);
	dstr_printf(&layout, "%s/g13.json", dir.array);

	json_error_t err;
	json_t *root = json_load_file(layout.array, 0, &err);
	if (!root) {
		blog(LOG_WARNING, "[g13pad-obs] %s: %s (line %d)", layout.array, err.text, err.line);
		goto done;
	}
	json_t *elements = json_object_get(root, "elements");
	json_t *w = json_object_get(root, "overlay_width"), *h = json_object_get(root, "overlay_height");
	if (!json_is_array(elements) || !json_is_number(w) || !json_is_number(h)) {
		blog(LOG_WARNING, "[g13pad-obs] %s: not a layout", layout.array);
		json_decref(root);
		goto done;
	}
	s->width = (uint32_t)json_number_value(w);
	s->height = (uint32_t)json_number_value(h);
	size_t n = json_array_size(elements);
	s->elements = bzalloc(n * sizeof(struct element));
	for (size_t i = 0; i < n; i++) {
		json_t *e = json_array_get(elements, i);
		json_t *pos = json_object_get(e, "pos"), *map = json_object_get(e, "mapping");
		const char *id = json_string_value(json_object_get(e, "id"));
		struct element *el = &s->elements[s->count];
		if (!id || !json_is_array(pos) || !json_is_array(map) || !number_at(pos, 0, &el->x) ||
		    !number_at(pos, 1, &el->y) || !number_at(map, 0, &el->sx) || !number_at(map, 1, &el->sy) ||
		    !number_at(map, 2, &el->w) || !number_at(map, 3, &el->h))
			continue;
		strncpy(el->id, id, sizeof(el->id) - 1);
		el->type = (int)json_integer_value(json_object_get(e, "type"));
		json_t *radius = json_object_get(e, "stick_radius");
		el->radius = json_is_number(radius) ? (float)json_number_value(radius) : 12.0f;
		s->count++;
	}
	json_decref(root);

	gs_image_file4_init(&s->sheet, png.array, GS_IMAGE_ALPHA_PREMULTIPLY);
	obs_enter_graphics();
	gs_image_file4_init_texture(&s->sheet);
	obs_leave_graphics();
	s->sheet_loaded = true;
	if (!s->sheet.image3.image2.image.loaded)
		blog(LOG_WARNING, "[g13pad-obs] %s: cannot load", png.array);
	else
		blog(LOG_INFO, "[g13pad-obs] sheet %s, %zu elements, %ux%u", dir.array, s->count, s->width, s->height);
done:
	dstr_free(&dir);
	dstr_free(&png);
	dstr_free(&layout);
}

// ---- the source ----

static const char *g13_get_name(void *unused)
{
	UNUSED_PARAMETER(unused);
	return obs_module_text("G13Pad");
}

static void g13_update(void *data, obs_data_t *settings)
{
	struct g13 *s = data;
	const char *dir = obs_data_get_string(settings, "sheet");
	bool lcd_only = obs_data_get_int(settings, "mode") == 1;
	int scale = (int)obs_data_get_int(settings, "lcd_scale");
	bool reload = !s->sheet_loaded || strcmp(dir ? dir : "", s->sheet_dir ? s->sheet_dir : "") != 0;
	bfree(s->sheet_dir);
	s->sheet_dir = bstrdup(dir);
	s->lcd_only = lcd_only;
	s->lcd_scale = scale < 1 ? 1 : scale > 16 ? 16 : scale;
	if (reload)
		sheet_load(s);
}

static void *g13_create(obs_data_t *settings, obs_source_t *source)
{
	struct g13 *s = bzalloc(sizeof(struct g13));
	s->source = source;
	s->stick[0] = s->stick[1] = 128;
	s->backlight[2] = 255;
	glass_load(s);
	g13_update(s, settings);
	poll_state(s);
	return s;
}

static void g13_destroy(void *data)
{
	struct g13 *s = data;
	sheet_free(s);
	obs_enter_graphics();
	gs_texture_destroy(s->lcd_tex);
	obs_leave_graphics();
	bfree(s->glass);
	bfree(s->sheet_dir);
	bfree(s);
}

static void g13_defaults(obs_data_t *settings)
{
	obs_data_set_default_string(settings, "sheet", "");
	obs_data_set_default_int(settings, "mode", 0);
	obs_data_set_default_int(settings, "lcd_scale", 4);
}

static obs_properties_t *g13_properties(void *unused)
{
	UNUSED_PARAMETER(unused);
	obs_properties_t *props = obs_properties_create();
	obs_property_t *mode = obs_properties_add_list(props, "mode", obs_module_text("Mode"), OBS_COMBO_TYPE_LIST,
						       OBS_COMBO_FORMAT_INT);
	obs_property_list_add_int(mode, obs_module_text("Mode.Pad"), 0);
	obs_property_list_add_int(mode, obs_module_text("Mode.Lcd"), 1);
	obs_properties_add_int_slider(props, "lcd_scale", obs_module_text("LcdScale"), 1, 16, 1);
	obs_properties_add_path(props, "sheet", obs_module_text("Sheet"), OBS_PATH_DIRECTORY, NULL, NULL);
	return props;
}

static uint32_t g13_width(void *data)
{
	struct g13 *s = data;
	return s->lcd_only ? LCD_W * (uint32_t)s->lcd_scale : s->width;
}

static uint32_t g13_height(void *data)
{
	struct g13 *s = data;
	return s->lcd_only ? LCD_H * (uint32_t)s->lcd_scale : s->height;
}

static void g13_tick(void *data, float seconds)
{
	struct g13 *s = data;
	s->since_poll += seconds;
	s->since_glass += seconds;
	if (s->since_poll >= POLL_SECONDS) {
		s->since_poll = 0;
		poll_state(s);
	}
	if (s->since_glass >= GLASS_SECONDS) {
		s->since_glass = 0;
		glass_load(s);
	}
}

static void draw(gs_eparam_t *image, gs_texture_t *tex, float x, float y, uint32_t sx, uint32_t sy, uint32_t w,
		 uint32_t h)
{
	gs_matrix_push();
	gs_matrix_translate3f(x, y, 0.0f);
	gs_effect_set_texture_srgb(image, tex);
	gs_draw_sprite_subregion(tex, 0, sx, sy, w, h);
	gs_matrix_pop();
}

static void g13_render(void *data, gs_effect_t *effect)
{
	struct g13 *s = data;
	if (s->lcd_dirty && s->have_lcd) {
		if (!s->lcd_tex)
			s->lcd_tex = gs_texture_create(LCD_W, LCD_H, GS_RGBA, 1, NULL, GS_DYNAMIC);
		if (s->lcd_tex)
			gs_texture_set_image(s->lcd_tex, s->lcd_rgba, LCD_W * 4, false);
		s->lcd_dirty = false;
	}
	const bool previous = gs_framebuffer_srgb_enabled();
	gs_enable_framebuffer_srgb(true);
	gs_blend_state_push();
	gs_blend_function(GS_BLEND_ONE, GS_BLEND_INVSRCALPHA);
	gs_eparam_t *image = gs_effect_get_param_by_name(effect, "image");

	if (s->lcd_only) {
		if (s->have_lcd && s->lcd_tex) {
			gs_matrix_push();
			gs_matrix_scale3f((float)s->lcd_scale, (float)s->lcd_scale, 1.0f);
			gs_effect_set_texture_srgb(image, s->lcd_tex);
			gs_draw_sprite(s->lcd_tex, 0, LCD_W, LCD_H);
			gs_matrix_pop();
		}
	} else if (s->sheet_loaded && s->sheet.image3.image2.image.texture) {
		gs_texture_t *tex = s->sheet.image3.image2.image.texture;
		bool clicked = held(s, "TOP");
		for (size_t i = 0; i < s->count; i++) {
			const struct element *e = &s->elements[i];
			if (strcmp(e->id, "lcd") == 0) {
				if (s->have_lcd && s->lcd_tex)
					draw(image, s->lcd_tex, e->x, e->y, 0, 0, LCD_W, LCD_H);
				continue;
			}
			bool pressed = e->type == KIND_KEY ? held(s, e->id) : e->type == KIND_STICK && clicked;
			float x = e->x, y = e->y;
			if (e->type == KIND_STICK) {
				x += (s->stick[0] - 127.5f) / 127.5f * e->radius;
				y += (s->stick[1] - 127.5f) / 127.5f * e->radius;
			}
			float sy = pressed ? e->sy + e->h + PRESSED_GAP : e->sy;
			draw(image, tex, x, y, (uint32_t)e->sx, (uint32_t)sy, (uint32_t)e->w, (uint32_t)e->h);
		}
	}

	gs_blend_state_pop();
	gs_enable_framebuffer_srgb(previous);
}

static struct obs_source_info g13_source = {
	.id = "g13pad",
	.type = OBS_SOURCE_TYPE_INPUT,
	.output_flags = OBS_SOURCE_VIDEO | OBS_SOURCE_SRGB,
	.get_name = g13_get_name,
	.create = g13_create,
	.destroy = g13_destroy,
	.update = g13_update,
	.get_defaults = g13_defaults,
	.get_properties = g13_properties,
	.get_width = g13_width,
	.get_height = g13_height,
	.video_tick = g13_tick,
	.video_render = g13_render,
	.icon_type = OBS_ICON_TYPE_GAME_CAPTURE,
};

bool obs_module_load(void)
{
	obs_register_source(&g13_source);
	blog(LOG_INFO, "[g13pad-obs] loaded (g13pad %s)", G13PAD_VERSION);
	return true;
}

const char *obs_module_name(void)
{
	return "g13pad-obs";
}

const char *obs_module_description(void)
{
	return "The Logitech G13 as a source: keys lit as pressed, the stick, the LCD, from g13pad's driver.";
}
