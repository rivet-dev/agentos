#include <ctype.h>
#ifdef isprint_l
#undef isprint_l
#endif
int (*foo)(int, locale_t) = isprint_l;
int main(void) { return 0; }
