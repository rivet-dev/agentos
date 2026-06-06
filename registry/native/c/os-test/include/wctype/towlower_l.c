#include <wctype.h>
#ifdef towlower_l
#undef towlower_l
#endif
wint_t (*foo)(wint_t, locale_t) = towlower_l;
int main(void) { return 0; }
