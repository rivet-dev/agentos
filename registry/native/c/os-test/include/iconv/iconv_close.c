#include <iconv.h>
#ifdef iconv_close
#undef iconv_close
#endif
int (*foo)(iconv_t) = iconv_close;
int main(void) { return 0; }
