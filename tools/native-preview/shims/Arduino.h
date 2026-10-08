#pragma once
#include <stdint.h>
#include <stddef.h>
#include <math.h>
#include <stdio.h>
#include <string.h>
#include <algorithm>
#include <string>
#include <new>
#include <type_traits>
#include <sstream>
#include <iomanip>
#define PROGMEM
#define IRAM_ATTR
#define F(value) value
using SemaphoreHandle_t = void *;
uint32_t millis();
inline void delay(uint32_t) {}
template <typename T, typename L, typename H> constexpr auto constrain(T v, L low, H high) {
  using V = typename std::common_type<T, L, H>::type;
  return std::max(V(low), std::min(V(v), V(high)));
}
using std::min;
using std::max;
struct SerialShim {
  void begin(uint32_t) {}
  template <typename... Args> void printf(const char *format, Args... args) { ::printf(format, args...); }
  void println(const char *message) { puts(message); }
};
inline SerialShim Serial;

class String {
 public:
  String() = default;
  String(const char *value) : text_(value ? value : "") {}
  String(const std::string &value) : text_(value) {}
  String(double value, unsigned char digits = 2) {
    std::ostringstream out; out << std::fixed << std::setprecision(digits) << value; text_ = out.str();
  }
  String(float value, unsigned char digits = 2) : String(double(value), digits) {}
  template <typename T, typename std::enable_if<std::is_integral<T>::value, int>::type = 0>
  String(T value) : text_(std::to_string(value)) {}
  const char *c_str() const { return text_.c_str(); }
  size_t length() const { return text_.size(); }
  bool isEmpty() const { return text_.empty(); }
  void reserve(size_t n) { text_.reserve(n); }
  char operator[](size_t i) const { return text_[i]; }
  void replace(const char *find, const char *replacement) {
    if (!find || !*find) return;
    const std::string from(find), to(replacement ? replacement : "");
    size_t position = 0;
    while ((position = text_.find(from, position)) != std::string::npos) {
      text_.replace(position, from.size(), to); position += to.size();
    }
  }
  String &operator+=(const String &other) { text_ += other.text_; return *this; }
  friend String operator+(String left, const String &right) { return left += right; }
  friend bool operator==(const String &left, const String &right) { return left.text_ == right.text_; }
  friend bool operator!=(const String &left, const String &right) { return !(left == right); }
 private:
  std::string text_;
};
