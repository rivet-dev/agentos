#include <uchar.h>
#ifdef c32rtomb
#undef c32rtomb
#endif
size_t (*foo)(char *restrict, char32_t, mbstate_t *restrict) = c32rtomb;
int main(void) { return 0; }
