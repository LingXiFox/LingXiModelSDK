package cn.lingxifox.modelsdk;

import cn.lingxifox.modelsdk.CatalogTypes.CatalogModel;
import cn.lingxifox.modelsdk.CatalogTypes.CatalogProvider;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;

public final class CatalogOrdering {
    private CatalogOrdering() {}

    private record Token(boolean isNumber, String raw, String trimmed) {}

    private static List<Token> tokenize(String text) {
        if (text == null || text.isEmpty()) return List.of();
        List<Token> tokens = new ArrayList<>();
        StringBuilder digits = new StringBuilder();
        StringBuilder words = new StringBuilder();

        for (int i = 0; i < text.length(); i++) {
            char ch = text.charAt(i);
            if (ch >= '0' && ch <= '9') {
                if (words.length() > 0) {
                    tokens.add(new Token(false, words.toString().toLowerCase(Locale.ROOT), ""));
                    words.setLength(0);
                }
                digits.append(ch);
            } else {
                if (digits.length() > 0) {
                    String raw = digits.toString();
                    String trimmed = raw.replaceFirst("^0+", "");
                    if (trimmed.isEmpty()) trimmed = "0";
                    tokens.add(new Token(true, raw, trimmed));
                    digits.setLength(0);
                }
                words.append(ch);
            }
        }
        if (digits.length() > 0) {
            String raw = digits.toString();
            String trimmed = raw.replaceFirst("^0+", "");
            if (trimmed.isEmpty()) trimmed = "0";
            tokens.add(new Token(true, raw, trimmed));
        }
        if (words.length() > 0) {
            tokens.add(new Token(false, words.toString().toLowerCase(Locale.ROOT), ""));
        }
        return tokens;
    }

    public static int compareNatural(String a, String b) {
        List<Token> left = tokenize(a == null ? "" : a);
        List<Token> right = tokenize(b == null ? "" : b);
        int limit = Math.min(left.size(), right.size());
        for (int i = 0; i < limit; i++) {
            Token lt = left.get(i);
            Token rt = right.get(i);
            if (lt.isNumber() && !rt.isNumber()) return -1;
            if (!lt.isNumber() && rt.isNumber()) return 1;
            if (lt.isNumber()) {
                int lenCmp = Integer.compare(lt.trimmed().length(), rt.trimmed().length());
                if (lenCmp != 0) return lenCmp;
                int valCmp = lt.trimmed().compareTo(rt.trimmed());
                if (valCmp != 0) return valCmp;
                int rawCmp = lt.raw().compareTo(rt.raw());
                if (rawCmp != 0) return rawCmp;
            } else {
                int txtCmp = lt.raw().compareTo(rt.raw());
                if (txtCmp != 0) return txtCmp;
            }
        }
        return Integer.compare(left.size(), right.size());
    }

    public static List<CatalogProvider> sortProviders(List<CatalogProvider> providers, List<String> featured) {
        Map<String, Integer> ranks = new HashMap<>();
        if (featured != null) {
            for (int i = 0; i < featured.size(); i++) {
                ranks.put(featured.get(i).trim().toLowerCase(Locale.ROOT), i);
            }
        }
        List<CatalogProvider> out = new ArrayList<>(providers);
        out.sort((a, b) -> {
            int ra = ranks.getOrDefault(a.id().trim().toLowerCase(Locale.ROOT), Integer.MAX_VALUE);
            int rb = ranks.getOrDefault(b.id().trim().toLowerCase(Locale.ROOT), Integer.MAX_VALUE);
            if (ra != rb) return Integer.compare(ra, rb);
            int byName = compareNatural(a.name(), b.name());
            if (byName != 0) return byName;
            return a.id().compareTo(b.id());
        });
        return List.copyOf(out);
    }

    public static List<CatalogModel> modelsInStableOrder(List<CatalogModel> models) {
        List<CatalogModel> out = new ArrayList<>(models);
        out.sort((a, b) -> {
            String ra = a.releaseDate() == null ? "" : a.releaseDate();
            String rb = b.releaseDate() == null ? "" : b.releaseDate();
            if (!ra.equals(rb)) {
                if (ra.isEmpty()) return 1;
                if (rb.isEmpty()) return -1;
                return rb.compareTo(ra);
            }
            int byFamily = compareNatural(a.family(), b.family());
            if (byFamily != 0) return byFamily;
            int byName = compareNatural(a.name(), b.name());
            if (byName != 0) return byName;
            return a.id().compareTo(b.id());
        });
        return List.copyOf(out);
    }
}
