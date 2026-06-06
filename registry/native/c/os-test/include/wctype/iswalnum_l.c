#include <wctype.h>
#ifdef iswalnum_l
#undef iswalnum_l
#endif
int (*foo)(wint_t, locale_t) = iswalnum_l;
int main(void) { return 0; }
