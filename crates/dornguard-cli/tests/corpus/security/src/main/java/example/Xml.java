package example;

import javax.xml.XMLConstants;
import javax.xml.parsers.DocumentBuilderFactory;
import javax.xml.parsers.SAXParserFactory;
import javax.xml.stream.XMLInputFactory;
import javax.xml.transform.TransformerFactory;

class Xml {
    Object unsafe() throws Exception {
        DocumentBuilderFactory documents = DocumentBuilderFactory.newInstance(); // expect: security.xxe
        SAXParserFactory sax = SAXParserFactory.newInstance(); // expect: security.xxe
        XMLInputFactory stax = XMLInputFactory.newFactory(); // expect: security.xxe
        TransformerFactory transformers = TransformerFactory.newInstance(); // expect: security.xxe
        // A feature set to the unsafe value does not protect the factory.
        DocumentBuilderFactory allowing = DocumentBuilderFactory.newInstance(); // expect: security.xxe
        allowing.setFeature("http://apache.org/xml/features/disallow-doctype-decl", false);
        return documents.newDocumentBuilder();
    }

    Object safe() throws Exception {
        DocumentBuilderFactory documents = DocumentBuilderFactory.newInstance();
        documents.setFeature("http://apache.org/xml/features/disallow-doctype-decl", true);

        SAXParserFactory sax;
        sax = SAXParserFactory.newInstance();
        sax.setFeature("http://apache.org/xml/features/disallow-doctype-decl", true);

        XMLInputFactory stax = XMLInputFactory.newInstance();
        stax.setProperty(XMLInputFactory.SUPPORT_DTD, false);

        TransformerFactory transformers = TransformerFactory.newInstance();
        transformers.setAttribute(XMLConstants.ACCESS_EXTERNAL_DTD, "");
        transformers.setAttribute(XMLConstants.ACCESS_EXTERNAL_STYLESHEET, "");
        return documents.newDocumentBuilder();
    }
}
