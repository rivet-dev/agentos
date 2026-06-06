#include <wctype.h>
#ifdef iswlower_l
#undef iswlower_l
#endif
int (*foo)(wint_t, locale_t) = iswlower_l;
int main(void) { return 0; }
