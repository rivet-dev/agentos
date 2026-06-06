#include <uchar.h>
#ifdef c16rtomb
#undef c16rtomb
#endif
size_t (*foo)(char *restrict, char16_t, mbstate_t *restrict) = c16rtomb;
int main(void) { return 0; }
