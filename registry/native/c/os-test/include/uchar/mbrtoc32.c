#include <uchar.h>
#ifdef mbrtoc32
#undef mbrtoc32
#endif
size_t (*foo)(char32_t *restrict, const char *restrict, size_t, mbstate_t *restrict) = mbrtoc32;
int main(void) { return 0; }
