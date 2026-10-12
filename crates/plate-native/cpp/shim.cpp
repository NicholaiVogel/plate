#include "plate-native/cpp/shim.h"
#include "plate-native/src/lib.rs.h"

#include <OpenColorIO/OpenColorIO.h>
#include <OpenImageIO/imageio.h>

#include <algorithm>
#include <iterator>
#include <vector>
#include <cctype>
#include <cmath>
#include <stdexcept>
#include <string>

namespace OCIO = OCIO_NAMESPACE;

namespace plate {

static std::string s(rust::Str v) { return std::string(v.data(), v.size()); }

std::unique_ptr<Config> config_from_file(rust::Str path) {
  auto c = std::make_unique<Config>();
  // CreateFromFile also resolves built-in URIs such as ocio://studio-config-latest.
  c->cfg = OCIO::Config::CreateFromFile(s(path).c_str());
  return c;
}

std::unique_ptr<Config> config_from_env() {
  auto c = std::make_unique<Config>();
  c->cfg = OCIO::Config::CreateFromEnv();
  return c;
}

rust::String config_name(const Config &cfg) {
  const char *n = cfg.cfg->getName();
  return rust::String(n ? n : "");
}

rust::String config_description(const Config &cfg) {
  const char *n = cfg.cfg->getDescription();
  return rust::String(n ? n : "");
}

rust::Vec<rust::String> colorspace_names(const Config &cfg) {
  rust::Vec<rust::String> out;
  int n = cfg.cfg->getNumColorSpaces(OCIO::SEARCH_REFERENCE_SPACE_ALL,
                                     OCIO::COLORSPACE_ACTIVE);
  for (int i = 0; i < n; ++i) {
    out.push_back(rust::String(cfg.cfg->getColorSpaceNameByIndex(
        OCIO::SEARCH_REFERENCE_SPACE_ALL, OCIO::COLORSPACE_ACTIVE, i)));
  }
  return out;
}

rust::String colorspace_family(const Config &cfg, rust::Str name) {
  auto cs = cfg.cfg->getColorSpace(s(name).c_str());
  if (!cs)
    return rust::String("");
  const char *f = cs->getFamily();
  return rust::String(f ? f : "");
}

rust::String colorspace_description(const Config &cfg, rust::Str name) {
  auto cs = cfg.cfg->getColorSpace(s(name).c_str());
  if (!cs)
    return rust::String("");
  const char *f = cs->getDescription();
  return rust::String(f ? f : "");
}

rust::String colorspace_encoding(const Config &cfg, rust::Str name) {
  auto cs = cfg.cfg->getColorSpace(s(name).c_str());
  if (!cs)
    return rust::String("");
  const char *e = cs->getEncoding();
  return rust::String(e ? e : "");
}

bool colorspace_is_data(const Config &cfg, rust::Str name) {
  auto cs = cfg.cfg->getColorSpace(s(name).c_str());
  return cs && cs->isData();
}

rust::String role_colorspace(const Config &cfg, rust::Str role) {
  auto cs = cfg.cfg->getColorSpace(s(role).c_str());
  if (!cs)
    return rust::String("");
  return rust::String(cs->getName());
}

rust::Vec<rust::String> displays(const Config &cfg) {
  rust::Vec<rust::String> out;
  for (int i = 0; i < cfg.cfg->getNumDisplays(); ++i)
    out.push_back(rust::String(cfg.cfg->getDisplay(i)));
  return out;
}

rust::String default_display(const Config &cfg) {
  return rust::String(cfg.cfg->getDefaultDisplay());
}

rust::Vec<rust::String> views(const Config &cfg, rust::Str display) {
  rust::Vec<rust::String> out;
  std::string d = s(display);
  for (int i = 0; i < cfg.cfg->getNumViews(d.c_str()); ++i)
    out.push_back(rust::String(cfg.cfg->getView(d.c_str(), i)));
  return out;
}

rust::String default_view(const Config &cfg, rust::Str display) {
  return rust::String(cfg.cfg->getDefaultView(s(display).c_str()));
}

static std::unique_ptr<Processor> finish(OCIO::ConstProcessorRcPtr proc) {
  auto p = std::make_unique<Processor>();
  p->noop = proc->isNoOp();
  p->cpu = proc->getOptimizedCPUProcessor(OCIO::BIT_DEPTH_F32, OCIO::BIT_DEPTH_F32,
                                          OCIO::OPTIMIZATION_DEFAULT);
  return p;
}

std::unique_ptr<Processor> display_processor(const Config &cfg, rust::Str src,
                                             rust::Str display, rust::Str view,
                                             float exposure_stops, float gamma) {
  auto dvt = OCIO::DisplayViewTransform::Create();
  dvt->setSrc(s(src).c_str());
  dvt->setDisplay(s(display).c_str());
  dvt->setView(s(view).c_str());

  auto pipeline = OCIO::LegacyViewingPipeline::Create();
  pipeline->setDisplayViewTransform(dvt);

  if (exposure_stops != 0.0f) {
    // Exposure is a gain applied in scene-linear, like the Nuke/RV viewer f-stop.
    double gain = std::pow(2.0, (double)exposure_stops);
    double m44[16] = {gain, 0, 0, 0, 0, gain, 0, 0, 0, 0, gain, 0, 0, 0, 0, 1};
    auto mtx = OCIO::MatrixTransform::Create();
    mtx->setMatrix(m44);
    pipeline->setLinearCC(mtx);
  }
  if (gamma > 0.0f && std::fabs(gamma - 1.0f) > 1e-5f) {
    // Viewer gamma is applied after the view transform in display space.
    double inv = 1.0 / (double)gamma;
    double v[4] = {inv, inv, inv, 1.0};
    auto exp = OCIO::ExponentTransform::Create();
    exp->setValue(v);
    exp->setNegativeStyle(OCIO::NEGATIVE_PASS_THRU);
    pipeline->setDisplayCC(exp);
  }

  return finish(pipeline->getProcessor(cfg.cfg, cfg.cfg->getCurrentContext()));
}

std::unique_ptr<Processor> colorspace_processor(const Config &cfg, rust::Str src,
                                                rust::Str dst) {
  return finish(cfg.cfg->getProcessor(s(src).c_str(), s(dst).c_str()));
}

void apply_rgba(const Processor &proc, rust::Slice<float> pixels, size_t width,
                size_t height) {
  if (pixels.size() < width * height * 4)
    throw std::runtime_error("apply_rgba: buffer smaller than width*height*4");
  OCIO::PackedImageDesc desc(pixels.data(), (long)width, (long)height, 4);
  proc.cpu->apply(desc);
}

bool processor_is_noop(const Processor &proc) { return proc.noop; }

static std::string lower(std::string v) {
  std::transform(v.begin(), v.end(), v.begin(),
                 [](unsigned char c) { return std::tolower(c); });
  return v;
}

static std::string format_name(OIIO::TypeDesc t) {
  switch (t.basetype) {
  case OIIO::TypeDesc::HALF: return "half";
  case OIIO::TypeDesc::FLOAT: return "float";
  case OIIO::TypeDesc::DOUBLE: return "float";
  case OIIO::TypeDesc::UINT8: return "8bit";
  case OIIO::TypeDesc::UINT16: return "16bit";
  case OIIO::TypeDesc::UINT32: return "32bit";
  default: return t.c_str();
  }
}

static std::unique_ptr<OIIO::ImageInput> open_or_throw(const std::string &path) {
  auto in = OIIO::ImageInput::open(path);
  if (!in)
    throw std::runtime_error("OpenImageIO: " + OIIO::geterror());
  return in;
}

static int bits_for(const OIIO::ImageSpec &spec) {
  int bits = spec.get_int_attribute("oiio:BitsPerSample", 0);
  if (bits == 0)
    bits = (int)spec.format.size() * 8;
  return bits;
}

OiioProbe oiio_probe(rust::Str path) {
  auto in = open_or_throw(s(path));
  const auto &spec = in->spec();
  OiioProbe p;
  p.width = (uint32_t)(spec.full_width > 0 ? spec.full_width : spec.width);
  p.height = (uint32_t)(spec.full_height > 0 ? spec.full_height : spec.height);
  p.channels = (uint32_t)spec.nchannels;
  p.format = rust::String(format_name(spec.format));
  p.bits = (uint32_t)bits_for(spec);
  p.colorspace = rust::String(spec.get_string_attribute("oiio:ColorSpace"));
  p.pixel_aspect = spec.get_float_attribute("PixelAspectRatio", 1.0f);
  p.has_alpha = spec.alpha_channel >= 0 || spec.nchannels == 2 || spec.nchannels == 4;
  p.fps = 0.0f;
  int fps[2] = {0, 0};
  if (spec.getattribute("FramesPerSecond", OIIO::TypeRational, fps) && fps[1] != 0)
    p.fps = (float)fps[0] / (float)fps[1];
  in->close();
  return p;
}

// Picks R/G/B/A by name (with or without a layer prefix), else positionally.
static void pick_channels(const OIIO::ImageSpec &spec, int idx[4]) {
  const int nch = spec.nchannels;
  idx[0] = idx[1] = idx[2] = idx[3] = -1;
  const char *want[4] = {"r", "g", "b", "a"};
  for (int c = 0; c < nch; ++c) {
    std::string n = lower(spec.channelnames[c]);
    auto dot = n.rfind('.');
    std::string tail = dot == std::string::npos ? n : n.substr(dot + 1);
    for (int k = 0; k < 4; ++k)
      if (idx[k] < 0 && tail == want[k])
        idx[k] = c;
  }
  if (idx[0] < 0 || idx[1] < 0 || idx[2] < 0) {
    if (nch >= 3) {
      idx[0] = 0; idx[1] = 1; idx[2] = 2;
      if (idx[3] < 0 && nch >= 4) idx[3] = 3;
    } else {
      idx[0] = idx[1] = idx[2] = 0;
      if (idx[3] < 0 && nch == 2) idx[3] = 1;
    }
  }
  if (spec.alpha_channel >= 0 && idx[3] < 0)
    idx[3] = spec.alpha_channel;
}

// Copies the data window into a display-window-sized RGBA buffer. Outside the
// data window stays as the caller initialised it (transparent black).
template <typename T>
static void place_rgba(const OIIO::ImageSpec &spec, const std::vector<T> &src,
                       const int idx[4], T one, T *out, int fw, int fh) {
  const int w = spec.width, h = spec.height, nch = spec.nchannels;
  const int ox = spec.x - spec.full_x, oy = spec.y - spec.full_y;
  for (int y = 0; y < h; ++y) {
    int ty = y + oy;
    if (ty < 0 || ty >= fh) continue;
    for (int x = 0; x < w; ++x) {
      int tx = x + ox;
      if (tx < 0 || tx >= fw) continue;
      const T *px = &src[((size_t)y * w + x) * nch];
      T *o = &out[((size_t)ty * fw + tx) * 4];
      o[0] = px[idx[0]];
      o[1] = px[idx[1]];
      o[2] = px[idx[2]];
      o[3] = idx[3] >= 0 ? px[idx[3]] : one;
    }
  }
}

OiioReadInfo oiio_read_rgba_half(rust::Str path, rust::Slice<uint16_t> out,
                                 uint32_t width, uint32_t height) {
  auto in = open_or_throw(s(path));
  const auto &spec = in->spec();
  const int w = spec.width, h = spec.height, nch = spec.nchannels;
  const int fw = spec.full_width > 0 ? spec.full_width : w;
  const int fh = spec.full_height > 0 ? spec.full_height : h;
  if ((uint32_t)fw != width || (uint32_t)fh != height)
    throw std::runtime_error("image size differs from the rest of the sequence");
  if (out.size() < (size_t)fw * fh * 4)
    throw std::runtime_error("output buffer too small");

  std::vector<uint16_t> src((size_t)w * h * nch);
  if (!in->read_image(0, 0, 0, nch, OIIO::TypeDesc::HALF, src.data()))
    throw std::runtime_error("OpenImageIO: " + in->geterror());
  in->close();

  int idx[4];
  pick_channels(spec, idx);
  const uint16_t half_one = 0x3C00;
  place_rgba<uint16_t>(spec, src, idx, half_one, out.data(), fw, fh);

  OiioReadInfo info;
  info.has_alpha = idx[3] >= 0;
  info.pixel_aspect = spec.get_float_attribute("PixelAspectRatio", 1.0f);
  return info;
}

rust::Vec<rust::String> oiio_extensions() {
  rust::Vec<rust::String> out;
  // Format: "openexr:exr,sxr,mxr;tiff:tif,tiff,..."
  std::string list = OIIO::get_string_attribute("extension_list");
  size_t pos = 0;
  while (pos < list.size()) {
    size_t end = list.find(';', pos);
    if (end == std::string::npos) end = list.size();
    std::string entry = list.substr(pos, end - pos);
    auto colon = entry.find(':');
    if (colon != std::string::npos) {
      std::string exts = entry.substr(colon + 1);
      size_t p = 0;
      while (p < exts.size()) {
        size_t e = exts.find(',', p);
        if (e == std::string::npos) e = exts.size();
        out.push_back(rust::String(exts.substr(p, e - p)));
        p = e + 1;
      }
    }
    pos = end + 1;
  }
  return out;
}

} // namespace plate
