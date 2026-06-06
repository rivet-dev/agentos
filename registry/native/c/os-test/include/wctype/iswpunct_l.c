#include <wctype.h>
#ifdef iswpunct_l
#undef iswpunct_l
#endif
int (*foo)(wint_t, locale_t) = iswpunct_l;
int main(void) { return 0; }
