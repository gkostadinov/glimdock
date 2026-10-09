/* Keep all fonts, colors and rendering settings shared with the physical board. */
#ifndef __ASSEMBLY__
#include <stdlib.h>
#include <stdio.h>
#endif
#include "../../firmware/include/lv_conf.h"
#define LV_ASSERT_HANDLER fprintf(stderr, "LVGL assertion at %s:%d\n", __FILE__, __LINE__); abort();
