#include <wctype.h>
#ifdef iswprint_l
#undef iswprint_l
#endif
int (*foo)(wint_t, locale_t) = iswprint_l;
int main(void) { return 0; }
