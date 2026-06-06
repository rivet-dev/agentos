#include <iconv.h>
#ifdef iconv
#undef iconv
#endif
size_t (*foo)(iconv_t, char **restrict, size_t *restrict, char **restrict, size_t *restrict) = iconv;
int main(void) { return 0; }
