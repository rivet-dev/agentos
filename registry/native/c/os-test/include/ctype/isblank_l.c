#include <ctype.h>
#ifdef isblank_l
#undef isblank_l
#endif
int (*foo)(int, locale_t) = isblank_l;
int main(void) { return 0; }
