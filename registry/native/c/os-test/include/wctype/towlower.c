#include <wctype.h>
#ifdef towlower
#undef towlower
#endif
wint_t (*foo)(wint_t) = towlower;
int main(void) { return 0; }
