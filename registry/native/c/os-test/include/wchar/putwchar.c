#include <wchar.h>
#ifdef putwchar
#undef putwchar
#endif
wint_t (*foo)(wchar_t) = putwchar;
int main(void) { return 0; }
