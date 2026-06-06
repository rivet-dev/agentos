#include <wctype.h>
#ifdef iswctype_l
#undef iswctype_l
#endif
int (*foo)(wint_t, wctype_t, locale_t) = iswctype_l;
int main(void) { return 0; }
