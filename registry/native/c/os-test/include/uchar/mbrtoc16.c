#include <uchar.h>
#ifdef mbrtoc16
#undef mbrtoc16
#endif
size_t (*foo)(char16_t *restrict, const char *restrict, size_t, mbstate_t *restrict) = mbrtoc16;
int main(void) { return 0; }
