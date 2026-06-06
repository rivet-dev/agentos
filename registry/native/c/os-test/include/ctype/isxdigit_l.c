#include <ctype.h>
#ifdef isxdigit_l
#undef isxdigit_l
#endif
int (*foo)(int, locale_t) = isxdigit_l;
int main(void) { return 0; }
