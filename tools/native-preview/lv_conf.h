/* Use the exact firmware rendering configuration; fail fast on desktop errors. */
#ifndef __ASSEMBLY__
#include <stdlib.h>
#include <stdio.h>
#endif
#include "../../firmware/include/lv_conf.h"
#define LV_ASSERT_HANDLER fprintf(stderr, "LVGL assertion at %s:%d\n", __FILE__, __LINE__); abort();
