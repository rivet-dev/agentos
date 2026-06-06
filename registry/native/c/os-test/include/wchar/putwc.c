#include <wchar.h>
#ifdef putwc
#undef putwc
#endif
wint_t (*foo)(wchar_t, FILE *) = putwc;
int main(void) { return 0; }
