#include <wctype.h>
#ifdef towctrans_l
#undef towctrans_l
#endif
wint_t (*foo)(wint_t, wctrans_t, locale_t) = towctrans_l;
int main(void) { return 0; }
