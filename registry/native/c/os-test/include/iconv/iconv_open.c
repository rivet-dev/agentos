#include <iconv.h>
#ifdef iconv_open
#undef iconv_open
#endif
iconv_t (*foo)(const char *, const char *) = iconv_open;
int main(void) { return 0; }
