#include <wctype.h>
#ifdef towctrans
#undef towctrans
#endif
wint_t (*foo)(wint_t, wctrans_t) = towctrans;
int main(void) { return 0; }
