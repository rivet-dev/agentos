#include <ctype.h>
#ifdef isalnum_l
#undef isalnum_l
#endif
int (*foo)(int, locale_t) = isalnum_l;
int main(void) { return 0; }
