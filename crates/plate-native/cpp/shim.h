#pragma once
#include <memory>
#include "rust/cxx.h"
#include <OpenColorIO/OpenColorIO.h>

namespace plate {

struct OiioProbe;
struct OiioReadInfo;

class Config {
public:
  OCIO_NAMESPACE::ConstConfigRcPtr cfg;
};

class Processor {
public:
  OCIO_NAMESPACE::ConstCPUProcessorRcPtr cpu;
  bool noop = false;
};

std::unique_ptr<Config> config_from_file(rust::Str path);
std::unique_ptr<Config> config_from_env();
rust::String config_name(const Config &cfg);
rust::String config_description(const Config &cfg);
rust::Vec<rust::String> colorspace_names(const Config &cfg);
rust::String colorspace_family(const Config &cfg, rust::Str name);
rust::String colorspace_description(const Config &cfg, rust::Str name);
rust::String colorspace_encoding(const Config &cfg, rust::Str name);
bool colorspace_is_data(const Config &cfg, rust::Str name);
rust::String role_colorspace(const Config &cfg, rust::Str role);
rust::Vec<rust::String> displays(const Config &cfg);
rust::String default_display(const Config &cfg);
rust::Vec<rust::String> views(const Config &cfg, rust::Str display);
rust::String default_view(const Config &cfg, rust::Str display);

std::unique_ptr<Processor> display_processor(const Config &cfg, rust::Str src,
                                             rust::Str display, rust::Str view,
                                             float exposure_stops, float gamma);
std::unique_ptr<Processor> colorspace_processor(const Config &cfg, rust::Str src,
                                                rust::Str dst);
void apply_rgba(const Processor &proc, rust::Slice<float> pixels, size_t width,
                size_t height);
bool processor_is_noop(const Processor &proc);

OiioProbe oiio_probe(rust::Str path);
OiioReadInfo oiio_read_rgba_half(rust::Str path, rust::Slice<uint16_t> out, uint32_t width,
                                 uint32_t height);
rust::Vec<rust::String> oiio_extensions();

} // namespace plate
