#include <wchar.h>
#ifdef wcsxfrm_l
#undef wcsxfrm_l
#endif
size_t (*foo)(wchar_t *restrict, const wchar_t *restrict, size_t, locale_t) = wcsxfrm_l;
int main(void) { return 0; }
